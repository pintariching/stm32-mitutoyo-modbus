#![deny(unsafe_code)]
#![no_main]
#![no_std]

use byteorder::{ByteOrder, NetworkEndian};
use heapless::Vec;
// Print panic message to probe console
use panic_probe as _;
use rmodbus::{
    server::{storage::ModbusStorage, ModbusFrame},
    ModbusProto,
};
use setup::{setup_peripherals, Gpio};
use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    socket::tcp::{Socket, SocketBuffer},
    time::Instant,
    wire::{EthernetAddress, Ipv4Address, Ipv4Cidr},
};
use stm32f4xx_hal::{interrupt, timer::SysTimerExt};

use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::{entry, exception};
use stm32_eth::{
    dma::{RxRingEntry, TxRingEntry},
    stm32::{CorePeripherals, Peripherals, SYST},
    EthPins, Parts,
};

mod mitutoyo;
mod setup;

const IP_ADDRESS: Ipv4Address = Ipv4Address::new(192, 168, 1, 200);
const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
const PORT: u16 = 502;

static TIME: Mutex<RefCell<u64>> = Mutex::new(RefCell::new(0));
static ETH_PENDING: Mutex<RefCell<bool>> = Mutex::new(RefCell::new(false));
static MODBUS_CONTEXT: Mutex<RefCell<ModbusStorage<8, 8, 8, 8>>> =
    Mutex::new(RefCell::new(ModbusStorage::new()));

#[allow(clippy::empty_loop)]
#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap();
    let mut cp = CorePeripherals::take().unwrap();

    let (clocks, gpio, ethernet) = setup_peripherals(p);
    let Gpio {
        gpioa,
        gpiob,
        gpioc,
        gpiog,
    } = gpio;

    let eth_pins = EthPins {
        ref_clk: gpioa.pa1.into_floating_input(),
        crs: gpioa.pa7.into_floating_input(),
        tx_en: gpiog.pg11.into_floating_input(),
        tx_d0: gpiog.pg13.into_floating_input(),
        tx_d1: gpiob.pb13.into_floating_input(),
        rx_d0: gpioc.pc4.into_floating_input(),
        rx_d1: gpioc.pc5.into_floating_input(),
    };

    let led = gpioc.pc13.into_push_pull_output();

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

    let mut server_rx_buffer = [0; 2048];
    let mut server_tx_buffer = [0; 2048];
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
            if let Err(e) = socket.listen(PORT) {
                defmt::error!("TCP listen error: {:?}", e)
            } else {
                defmt::info!("Listening at {}:{}...", IP_ADDRESS, PORT);
            }
        }

        cortex_m::interrupt::free(|cs| {
            let mut context = MODBUS_CONTEXT.borrow(cs).borrow_mut();
            let time_bytes = time.to_be_bytes();
            defmt::info!("Time is: {}", time_bytes);
            context.inputs[0] = NetworkEndian::read_u16(&time_bytes[6..8]);
        });

        if socket.is_open() {
            let mut data = [0; 256];
            let mut response: Vec<u8, 256> = Vec::new();

            if socket.can_recv() {
                let _recv_len = match socket.recv_slice(&mut data) {
                    Ok(len) => len,
                    Err(e) => {
                        defmt::error!("{}", e);
                        continue;
                    }
                };
            } else {
                continue;
            };

            let mut frame = ModbusFrame::new(1, &mut data, ModbusProto::TcpUdp, &mut response);

            if frame.parse().is_err() {
                defmt::error!("Failed to parse modbus frame")
            };

            if frame.processing_required {
                let result = cortex_m::interrupt::free(|cs| {
                    let mut context = MODBUS_CONTEXT.borrow(cs).borrow_mut();

                    if frame.readonly {
                        frame.process_read(&*context)
                    } else {
                        frame.process_write(&mut *context)
                    }
                });

                if result.is_err() {
                    defmt::error!("Failed to read or write when processing");
                }

                if frame.response_required {
                    if frame.finalize_response().is_err() {
                        defmt::error!("Failed to finalize response");
                    }

                    let response = response.as_slice();

                    if let Err(e) = socket.send_slice(response) {
                        defmt::error!("{}", e);
                    }
                }
            }
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
