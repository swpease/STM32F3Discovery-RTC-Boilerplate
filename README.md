# Annotated RTC Initialization

Annotation / boilerplate for RTC initialization on an STM32F3DISCOVERY board. It is configured to go into `Stop` mode, and uses the `Periodic auto-wakeup` feature. This project was initialized using [cortex-m-quickstart](https://github.com/rust-embedded/cortex-m-quickstart).

To use this, you need to put your interrupt-handling logic (what you want to have happen when the wakeup timer goes off) in the interrupt. And probably add in some safety measures.

The tricky thing about this initialization is that it requires intermingling calls to several register blocks: `RTC`, `RCC`, `PWR`, `EXTI`, and core registers `NVIC` and `SCB`.

# Annotation by Section (see `main.rs`)
## Preamble
To use `Stop` or `Standby` mode, you need to use a low-speed clock (LSI or LSE). The Discovery board only uses LSI, so we'll use that. LSE is more accurate, but here we are.
  - set LSION in RCC.CSR
  - wait for it to be ready (LSIRDY)

System resets cause PWR.CR.DBP to be unset. DBP is required to write to RTC stuff, which is kinda important for initializing RTC.
  - enable PWR via RCC.APB1ENR
  - set PWR.CR.DBP
  - reset RTC domain if desired (see below) 
    - RTC.WPR unlock required afterwards

Because the RTC domain needs to be operable in low-power conditions, it has separate resetting etc. behavior. To reset this domain, use BDRST (backup domain reset) in RCC.BDCR. This is at least useful for playing around in development. *This requires DBP to be set beforehand.* Doing this means you'll have to perform more steps to get write access to RTC (see Init).
  - RCC.BDCR.BDRST set,unset

## Init
Now that we have DBP set, we can set-up the RTC clocks in RCC.BDCR
  - enable RTCEN
    - just like we do with any other peripheral.
  - set our RTC clock as LSI

If you want to make sure you're not stopped by any Backup domain reset, do the above-mentioned RTC.WPR unlock:
  - write 0xCA, then 0x53
    - I put this after enabling the RTC clock in RCC.BDCR, which I assume is required.

### RTC initialization:
Initialization basically consists of setting up an initial date and time for your RTC, as well as setting up a 1 Hz clock. This is wrapped within an INIT bit set / unset.
  - RTC.ISR.INIT set bit
    - wait until INITF bit is set

We need to configure the two prediv's to yield a 1 Hz (i.e. prediv_a * prediv_s = LSI speed (or LSE if you're using, but then you don't need to configure assuming you're at 128*256 Hz))
  - RTC.PRER.PREDIV_S = 312
    - (312 + 1) * 128 ~ 40_000 (i.e. 40 kHz)

Set RTC.TR and RTC.DR to whatever you want (time and date initial values)
  - RTC.TR set
  - RTC.DR set

Finally, we're done initializing, so:
  - RTC.ISR.INIT clear bit

## Interrupt Config [ref: 27.5]
We need to make sure:
  - RTC generates an interrupt
  - EXTI is hooked up to receive and propogate said interrupt
    - we're dealing with a hardware interrupt here
  - NVIC is set to receive EXTI's propogated interrupt
  
RTC wakeup is connected to EXTI line 20, so we'll configure bit 20 of EXTI:
  - EXTI.IMR1.MR20 set bit
  - EXTI.RTSR1.TR20 set bit
    - The manual says to configure for rising edge pulse detection. Who am I to argue.

We also need to make sure that NVIC (in Cortex-M) is setup to hear these interrupts:
  - NVIC unmask RTC_WKUP

We also need to make sure that RTC generates an Interrupt:
  - RTC.CR.WUTIE set bit

## Low-Power Mode
I want to use Stop mode, not Standby mode, because Standby clears out SRAM and the registers, which I don't want. It seems to me like Standby is practically just Power-on, or a System Reset (barring Backup domain / RTC stuff, of course), since it starts over from the top (I'm pretty sure). Stop mode is in line with what I'm looking for with like a periodic sensor or some such. Configuration is pretty straightforward, and follows 7.3.5.
  - PWR.CR.CSBF set
  - PWR.CR.LPDS set
  - SCB set SLEEPDEEP (Cortex-M)

To actually enter Stop mode, we need to call `wfi`. Which we'll do when we want to (in the loop).

## Debug settings
It looks like something already handles this, but I had it set in DBGMCU to make sure that (a) you could debug in low-power modes, and (b) the RTC countdown would freeze when you were paused in debugging (core halted).

## Start
The RTC wakeup timer starts counting as soon as we set RTC.CR.WUTE, so we do that right before entering our loop:
  - RTC.CR.WUTE set

In the loop, we do a couple of things: (a) clear a flag (see below, WUTF), and (b) actually enter our low-power mode.
  - loop:
    - RTC.ISR.WUTF clear
    - call `wfi`

## Interrupt Handling
We need to clear a couple of bits post-interrupt to keep things running properly
  - EXTI.PR1.PR20 set (to clear pending-ness)
    - otherwise, it'll continously fire the interrupt handler
      - ergo, need to do this *in* the handler
  - RTC.ISR.WUTF clear
    - need to do this before wakeup timer reaches zero again
      - can do this in the loop (easier than worrying about concurrency)