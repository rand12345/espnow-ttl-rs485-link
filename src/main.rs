use embedded_svc::wifi::{AccessPointConfiguration, Configuration, Wifi};
use esp_idf_hal::{prelude::*, serial};
use esp_idf_svc::{
    espnow::{EspNowClient, PeerInfo},
    netif::EspNetifStack,
    nvs::EspDefaultNvs,
    sysloop::EspSysLoopStack,
    wifi::EspWifi,
};
use esp_idf_sys as _;
use futures_micro::yield_once as yield_now;
use log::*;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

// Async sleep helper func
async fn sleep(dur: Duration) -> usize {
    let when = Instant::now() + dur;
    let mut count = 0;
    loop {
        if Instant::now() >= when {
            break count;
        } else {
            count += 1;
            yield_now().await;
        }
    }
}

// UART Rx routine
// Small buffer for stack, outputs smaller non-padded heap Vec of actual payload size
async fn wait_uart_rx(rx: &mut serial::Rx<serial::UART1>) -> anyhow::Result<Vec<u8>> {
    loop {
        let mut buf = [0u8; 256];
        if let Ok(len) = rx.read_bytes_blocking(&mut buf, Duration::from_micros(10_000)) {
            if len > 0 {
                let vecbuf = buf.into_iter().take(len).collect::<Vec<u8>>();
                info!("UART1 Rx: {:?}", &vecbuf);
                return Ok(vecbuf);
            }
        } else {
            sleep(Duration::from_millis(1)).await;
        }
    }
}

fn main() {
    esp_idf_sys::link_patches();
    // MAC address of each device
    let _rs485: [u8; 6] = [104, 103, 37, 130, 63, 229];
    let _ttl: [u8; 6] = [104, 103, 37, 130, 176, 157];

    // switch these around for each device
    let remote = _ttl;
    let _local = _rs485;
    let ssid = "RS485link RS485";
    // let ssid = "RS485link TTL"; // uncomment for TTL node

    // UART baudrate
    let uart_baud = Hertz(9600);

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let hal = esp_idf_hal::peripherals::Peripherals::take().unwrap();

    // UART HAL setup
    let config = serial::config::Config::default().baudrate(uart_baud);
    let _userial: serial::Serial<serial::UART1, _, _> = serial::Serial::new(
        hal.uart1,
        serial::Pins {
            tx: hal.pins.gpio19,
            rx: hal.pins.gpio18,
            cts: None,
            rts: None,
        },
        config,
    )
    .unwrap();

    // Seperate UART channels
    let (mut tx, mut rx) = _userial.split();

    // WiFi hardware configuration
    let netif_stack = Arc::new(EspNetifStack::new().unwrap());
    let sys_loop_stack = Arc::new(EspSysLoopStack::new().unwrap());
    let default_nvs = Arc::new(EspDefaultNvs::new().unwrap());

    let mut wifi = EspWifi::new(netif_stack, sys_loop_stack, default_nvs).unwrap();
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ssid.into(),
        ..Default::default()
    }))
    .unwrap();

    // ESP Now configuration
    let espnow_a = Arc::new(EspNowClient::new().unwrap());

    {
        // Add remote device by MAC address
        let espnow = espnow_a.clone();
        espnow
            .add_peer(PeerInfo {
                peer_addr: remote,
                ifidx: 1,
                ..Default::default()
            })
            .unwrap();

        // Send ESP Now Rx bytes to UART Tx as callback func
        espnow
            .register_recv_cb(move |_addr, _dat| {
                tx.write_bytes(_dat).unwrap();
                info!("ESPNow RX: addr {:02x?} dat {:02x?}", _addr, _dat);
            })
            .unwrap();

        // Debug on boot
        println!("Number of Peer: {:?}", espnow.get_peers_number());
        println!("Get Peers: {:?}", espnow.get_peer(remote));
        println!("Remote Peer: {:?}", espnow.peer_exists(remote));
    }

    // Send UART Rx bytes to ESP Now Tx
    let espnow = espnow_a;
    let uart_rx_to_espnow_tx = async move {
        loop {
            let buf = wait_uart_rx(&mut rx).await.unwrap();
            match espnow.send(remote, &buf) {
                Ok(_) => info!("ESPNow TX: {:02x?} {:02x?}", remote, &buf),
                Err(e) => eprint!("{:02x?}", e),
            }
        }
    };

    // Background busy loop
    let sleep_task = async {
        loop {
            std::thread::sleep(Duration::from_millis(1));
            yield_now().await
        }
    };

    // Wait for wifi to init before launching async futures
    std::thread::sleep(Duration::from_secs(1));
    spin_on::spin_on(futures_micro::zip!(sleep_task, uart_rx_to_espnow_tx,));
}
