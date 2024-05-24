use byteorder::{ByteOrder, NetworkEndian};
use micromath::F32Ext;
use stm32f4xx_hal::gpio::{Input, Output, Pin};

use crate::{MODBUS_CONTEXT, SET_ORIGIN_TIMEOUT, TIME};

pub enum MeasurementError {
    Timeout,
    InvalidStart,
}

pub struct Urica<
    const DP: char,
    const DN: u8,
    const CP: char,
    const CN: u8,
    const RP: char,
    const RN: u8,
    const OP: char,
    const ON: u8,
> {
    data: Pin<DP, DN, Input>,
    clock: Pin<CP, CN, Input>,
    request: Pin<RP, RN, Output>,
    origin: Pin<OP, ON, Output>,
    origin_coil: usize,
    origin_set_start: u64,
    setting_origin: bool,
    measurement_register: usize,
}

impl<
        const DP: char,
        const DN: u8,
        const CP: char,
        const CN: u8,
        const RP: char,
        const RN: u8,
        const OP: char,
        const ON: u8,
    > Urica<DP, DN, CP, CN, RP, RN, OP, ON>
{
    pub fn new(
        data: Pin<DP, DN, Input>,
        clock: Pin<CP, CN, Input>,
        request: Pin<RP, RN, Output>,
        origin: Pin<OP, ON, Output>,
        origin_coil: usize,
        measurement_register: usize,
    ) -> Self {
        Self {
            data,
            clock,
            request,
            origin,
            origin_coil,
            origin_set_start: 0,
            setting_origin: false,
            measurement_register,
        }
    }

    pub fn measure(&mut self) -> Result<f32, MeasurementError> {
        let timeout = 40;
        let mut data = [0u8; 13];

        self.request.set_high();

        for i in 0..13 {
            let mut k = 0u8;

            for _ in 0..4 {
                let start_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
                while self.clock.is_low() {
                    // defmt::debug!("Waiting for clock to turn high");
                    let current_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());

                    if current_time - start_time > timeout {
                        return Err(MeasurementError::Timeout);
                    }
                }

                let start_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
                while self.clock.is_high() {
                    //defmt::debug!("Waiting for clock to turn low");
                    let current_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());

                    if current_time - start_time > timeout {
                        return Err(MeasurementError::Timeout);
                    }
                }

                let bit = (self.data.is_high() as u8) << 3;
                k >>= 1;
                k = k | bit;
            }

            if i == 0 {
                self.request.set_low();
            }

            data[i] = k
        }

        for i in 0..4 {
            if data[i] != 0xf {
                return Err(MeasurementError::InvalidStart);
            }
        }

        let decimal = data[11];
        let mut value: f32 = 0.;

        for i in 0..6 {
            let mut data_buf_float = data[i + 5] as f32;
            data_buf_float *= 10f32.powi(5 - i as i32);
            value += data_buf_float;
        }

        value /= 10f32.powi(decimal as i32);

        if data[4] == 0x8 {
            value *= -1.;
        }

        Ok(value)
    }

    pub fn poll(&mut self) {
        let set_origin = cortex_m::interrupt::free(|cs| {
            let modbus = MODBUS_CONTEXT.borrow(cs).borrow();
            modbus.coils[self.origin_coil as usize]
        });

        if set_origin & (self.setting_origin == false) {
            self.setting_origin = true;
            self.origin.set_high();
            self.origin_set_start = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
        }

        if set_origin & self.setting_origin {
            let current_time = cortex_m::interrupt::free(|cs| *TIME.borrow(cs).borrow());
            if current_time - self.origin_set_start > SET_ORIGIN_TIMEOUT {
                self.origin.set_low();
                self.setting_origin = false;
                cortex_m::interrupt::free(|cs| {
                    let mut modbus = MODBUS_CONTEXT.borrow(cs).borrow_mut();
                    modbus.coils[self.origin_coil as usize] = false;
                })
            }
        }

        if !set_origin & !self.setting_origin {
            match self.measure() {
                Ok(mera) => cortex_m::interrupt::free(|cs| {
                    let mut modbus = MODBUS_CONTEXT.borrow(cs).borrow_mut();

                    let mera_bytes = mera.to_be_bytes();
                    modbus.inputs[self.measurement_register + 1] =
                        NetworkEndian::read_u16(&mera_bytes[0..2]);
                    modbus.inputs[self.measurement_register] =
                        NetworkEndian::read_u16(&mera_bytes[2..4]);
                }),
                Err(_) => (),
            }
        }
    }
}
