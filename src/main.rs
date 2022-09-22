// use embedded_hal::digital::v2::InputPin; //, OutputPin, ToggleableOutputPin};

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
    // fmt::Debug,
    sync::Arc,
    time::{Duration, Instant},
};

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

// async fn send_uart_tx(// tx: &mut serial::Tx<serial::UART1>,
//     // vecbuf: &mut Vec<u8>,
// ) -> anyhow::Result<()> {
//     Ok(())
// loop {
// let mut buf = [0u8; 256];
// if let Ok(len) = rx.read_bytes_blocking(&mut buf, Duration::from_micros(1000)) {
//     vecbuf.clear();
//     vecbuf.extend(buf.iter().take(len));
// } else {
//     sleep(Duration::from_millis(1)).await;
// }
// }
// }

fn main() {
    esp_idf_sys::link_patches();
    let _rs485: [u8; 6] = [104, 103, 37, 130, 63, 229];
    let _ttl: [u8; 6] = [104, 103, 37, 130, 176, 157];

    let remote = _ttl;
    let local = _rs485;

    let ssid = "RS485link RS485";

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let hal = esp_idf_hal::peripherals::Peripherals::take().unwrap();

    let config = serial::config::Config::default().baudrate(Hertz(9600));
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

    let (mut tx, mut rx) = _userial.split();

    // let mut input_button = hal.pins.gpio3.into_input().unwrap();
    // let mut pin1 = hal.pins.gpio4.into_output().unwrap();
    // let mut rx_led_pin = hal.pins.gpio7.into_output().unwrap();

    let netif_stack = Arc::new(EspNetifStack::new().unwrap());
    let sys_loop_stack = Arc::new(EspSysLoopStack::new().unwrap());
    let default_nvs = Arc::new(EspDefaultNvs::new().unwrap());

    let mut wifi = EspWifi::new(netif_stack, sys_loop_stack, default_nvs).unwrap();
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ssid.into(),
        ..Default::default()
    }))
    .unwrap();

    // let (tx_now, rx_now) = async_channel::bounded::<()>(10);
    // let (tx_led, rx_led) = async_channel::bounded::<()>(10);

    let espnow_a = Arc::new(EspNowClient::new().unwrap());

    {
        let espnow = espnow_a.clone();
        espnow
            .add_peer(PeerInfo {
                peer_addr: remote,
                ifidx: 1,
                ..Default::default()
            })
            .unwrap();

        espnow
            .register_recv_cb(move |_addr, _dat| {
                tx.write_bytes(_dat).unwrap();
                info!("ESPNow RX: addr {:02x?} dat {:02x?}", _addr, _dat);
            })
            .unwrap();

        println!("Number of Peer: {:?}", espnow.get_peers_number());
        println!("Get Peers: {:?}", espnow.get_peer(remote));
        println!("Remote Peer: {:?}", espnow.peer_exists(remote));
    }

    // Sending byte task
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

    let sleep_task = async {
        loop {
            std::thread::sleep(Duration::from_millis(1));
            yield_now().await
        }
    };
    std::thread::sleep(Duration::from_secs(1));
    spin_on::spin_on(futures_micro::zip!(
        // print_task,
        sleep_task,
        uart_rx_to_espnow_tx,
        // task2,
        // task3
    ));
}

// register_recv_cb: addr [104, 103, 37, 130, 176, 157] dat [123]

// register_recv_cb: addr [104, 103, 37, 130, 63, 229] dat [123]
