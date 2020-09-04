#![no_std]
#![no_main]

use panic_halt as _; // you can put a breakpoint on `rust_begin_unwind` to catch panics

use cortex_m_rt::entry;
use cortex_m::asm;
use stm32f3_discovery::stm32f3xx_hal::{
    interrupt,
    stm32::{
        Peripherals,
        CorePeripherals,
        Interrupt,
        RCC,
        RTC,
        PWR,
        EXTI,
        NVIC,
        SCB,
        DBGMCU
    }
};

#[entry]
fn main() -> ! {
    // for gdb inspection
    let _rcc = RCC::ptr();
    let _rtc = RTC::ptr();
    let _pwr = PWR::ptr();
    let _exti = EXTI::ptr();
    let _nvic = NVIC::ptr();
    let _scb = SCB::ptr();
    let _debug = DBGMCU::ptr();
    // looks like the Backup Domain (ref Fig. 9) persists through unplugs from usb
    // Makes sense if you consider the conditions of Backup reset [ref 9.1.3]
    // RTC registers say what happens on System vs Backup resets
    // --> use `write` or explicitly reset Backup at start when using RTC, for safety

    let device_periphs = Peripherals::take().unwrap();
    let core_periphs = CorePeripherals::take().unwrap();
    let rcc = device_periphs.RCC;
    let realtime = device_periphs.RTC;
    let pwr = device_periphs.PWR;
    let exti = device_periphs.EXTI;
    let mut scb = core_periphs.SCB;
    
    let debug = device_periphs.DBGMCU;


    // PREAMBLE
    // want LSI clock enabled (don't have LSE or HSE on Discovery board)
    rcc.csr.modify(|_, w| { w.lsion().set_bit() });
    // Do I need this?
    while rcc.csr.read().lsirdy().bit_is_clear() {}
    // the LSI speed is given as a wide range (30-50 kHz). Calibration requires HSE.

    // enable PWR
    rcc.apb1enr.modify(|_, w| { w.pwren().set_bit() });
    // enable write access Part I [ref 9.4.9] (dbp reset by wakeup from Standby mode [ref 7.4.1])
    pwr.cr.write(|w| { w.dbp().set_bit() });

    // reset RTC domain (at least while prototyping):
    // need dbp set before doing this [ref: 9.4.9]
    rcc.bdcr.modify(|_, w| { w.bdrst().set_bit() });
    rcc.bdcr.modify(|_, w| { w.bdrst().clear_bit() });
    // END PREAMBLE 

    
    // INIT ref: 27.3.7
    // need dbp set before doing this [ref: 9.4.9]
    rcc.bdcr.write(|w| {
        w.rtcen().set_bit();  // enable RTC clock (for register writing, like other periphs)
        w.rtcsel().lsi()  // use LSI for RTC
    });

    // enable write status part II [ref 27.3.7, backup domain reset]
    realtime.wpr.write(|w| {
        unsafe { w.key().bits(0xCA) }
    });
    realtime.wpr.write(|w| {
        unsafe { w.key().bits(0x53) }
    });

    realtime.isr.modify(|_, w| { w.init().set_bit() });
    while realtime.isr.read().initf().bit_is_clear() {}
    // assuming 40 kHz LSI
    realtime.prer.write(|w| {
        unsafe { w.prediv_s().bits(312) }  // (312 + 1) * 128
    });
    // setting time to 22:39:10
    realtime.tr.modify(|_, w| {
        unsafe {
            w.ht().bits(2);
            w.hu().bits(2);
            w.mnt().bits(3);
            w.mnu().bits(9);
            w.st().bits(1)
        }
    });
    // date: Sept 3, 2020
    realtime.dr.modify(|_, w| {
        unsafe {
            w.yt().bits(2);
            w.mu().bits(9);
            w.du().bits(3)
        }
    });
    // 24 hr day default, so leave RTC_CR FMT as is

    realtime.isr.modify(|_, w| { w.init().clear_bit() });
    // END INIT


    // INTERRUPT CONFIG [ref: 27.5]
    exti.imr1.write(|w| w.mr20().set_bit());
    exti.rtsr1.write(|w| w.tr20().set_bit());
    unsafe { NVIC::unmask(Interrupt::RTC_WKUP); }

    // wakeup frequency
    realtime.wutr.write(|w| {
        unsafe { w.wut().bits(9) }  // using 1 Hz clock, so (9 + 1) seconds
    });  // starts when WUTE set to 1

    realtime.cr.write(|w| {
        unsafe { w.wcksel().bits(0b100); }  // use 1 Hz clock
        w.wutie().set_bit()  // to exit low power mode [ref 27.3.6]
    });
    // END INTERRUPT CONFIG


    // CONFIGURE SLEEPING [ref 7.3.5]
    pwr.cr.modify(|_, w| {
        w.csbf().set_bit();
        w.lpds().set_bit();  // Don't care about a few milliseconds on Wakeup; save power.
        w.pdds().clear_bit()  // Don't want Standby, want Stop.
        // Standby clears SRAM, registers, acts like a System Reset on wakeup
    });
    scb.set_sleepdeep();
    // END CONFIGURE SLEEPING


    // DEBUG ONLY
    // TODO: is this handled automatically?
    // lets you debug in stop mode
    debug.cr.modify(|_, w| w.dbg_stop().set_bit());
    // make RTC freeze when core halted
    debug.apb1_fz.modify(|_, w| w.dbg_rtc_stop().set_bit());
    // END DEBUG


    // start!
    realtime.cr.modify(|_, w| w.wute().set_bit());
    loop {
        // must clear wutf before next wakeup event [ref 27.3.6]
        realtime.isr.modify(|_, w| w.wutf().clear_bit());
        asm::wfi()
    }
}

#[interrupt]
fn RTC_WKUP() {
    // need to reset pending bit or will continuously refire
    unsafe {
        let exti = &(*EXTI::ptr());
        exti.pr1.modify(|_, w| w.pr20().set_bit());
    }
}