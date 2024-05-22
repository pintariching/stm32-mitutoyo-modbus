#![deny(unsafe_code)]
#![no_main]
#![no_std]

use heapless::Vec;
// Print panic message to probe console
use panic_probe as _;
use rmodbus::{client::ModbusRequest, ModbusProto};
use setup::{setup_peripherals, setup_pins};
use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    socket::tcp::{Socket, SocketBuffer},
    time::Instant,
    wire::{EthernetAddress, Ipv4Address, Ipv4Cidr},
};
use stm32f4xx_hal::interrupt;

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::{entry, exception};
use stm32_eth::{
    dma::{RxRingEntry, TxRingEntry},
    stm32::{CorePeripherals, Peripherals, SYST},
    Parts,
};

mod setup;

const IP_ADDRESS: Ipv4Address = Ipv4Address::new(192, 168, 1, 169);
const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

static TIME: Mutex<RefCell<u64>> = Mutex::new(RefCell::new(0));
static ETH_PENDING: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(false));

#[allow(clippy::empty_loop)]
#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap();
    let mut cp = CorePeripherals::take().unwrap();

    let (clocks, gpio, ethernet) = setup_peripherals(p);
    let eth_pins = setup_pins(gpio);

    let mut rx_ring: [RxRingEntry; 2] = Default::default();
    let mut tx_ring: [TxRingEntry; 2] = Default::default();

    let Parts {
        mut dma,
        mac: _,
        ptp: _,
    } = stm32_eth::new(
        ethernet,
        &mut rx_ring[..],
        &mut tx_ring[..],
        clocks,
        eth_pins,
    )
    .unwrap();
    dma.enable_interrupt();

    setup_systick(&mut cp.SYST);

    let eth_address = EthernetAddress(SRC_MAC);
    let config = Config::new(eth_address.into());
    let mut iface = Interface::new(config, &mut &mut dma, Instant::ZERO);

    iface.update_ip_addrs(|addr| {
        addr.push(smoltcp::wire::IpCidr::Ipv4(Ipv4Cidr::new(IP_ADDRESS, 24)))
            .ok();
    });

    let mut sockets = [SocketStorage::EMPTY];
    let mut sockets = SocketSet::new(&mut sockets[..]);

    let mut server_rx_buffer = [0; 1024];
    let mut server_tx_buffer = [0; 1024];
    let server_socket: Socket = Socket::new(
        SocketBuffer::new(&mut server_rx_buffer[..]),
        SocketBuffer::new(&mut server_tx_buffer[..]),
    );
    let server_handle = sockets.add(server_socket);

    loop {
        let time: u64 = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
        cortex_m::interrupt::free(|cs| {
            let mut eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
            *eth_pending = false;
        });

        iface.poll(
            Instant::from_millis(time as i64),
            &mut &mut dma,
            &mut sockets,
        );

        let socket = sockets.get_mut::<Socket>(server_handle);

        if !socket.is_listening() && !socket.is_open() {
            socket.abort();
            if let Err(e) = socket.listen(5502) {
                defmt::error!("TCP listen error: {:?}", e)
            } else {
                defmt::info!("Listening at {}:5502...", IP_ADDRESS);
            }
        } else {
            let mut mreq = ModbusRequest::new(1, ModbusProto::TcpUdp);

            let mut request: Vec<u8, 512> = Vec::new();
            mreq.generate_set_coils_bulk(0, &[true, true], &mut request);

            // match socket.recv(|data| (data.len(), data)) {
            //     Ok(d) => {
            //         let request = ModbusRequest::p
            //     },
            //     Err(e) => defmt::error!("{}", e),
            // }
        }
    }
}

fn setup_systick(syst: &mut SYST) {
    syst.set_reload(SYST::get_ticks_per_10ms() / 10);
    syst.enable_counter();
    syst.enable_interrupt();
}

#[exception]
fn SysTick() {
    cortex_m::interrupt::free(|cs| {
        let mut time = TIME.borrow(cs).borrow_mut();
        *time += 1;
    })
}

#[interrupt]
fn ETH() {
    cortex_m::interrupt::free(|cs| {
        let mut eth_pending = ETH_PENDING.borrow(cs).borrow_mut();
        *eth_pending = true;
    });

    stm32_eth::eth_interrupt_handler();
}
