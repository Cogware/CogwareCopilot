// SPDX-License-Identifier: GPL-3.0-only
//! Exhaustive catalogue policy, including healthy defaults and unfitted equipment.
//! EGT/knock 5–8, bank-two cams/injectors, DI and rotary ignition are not fitted.
//! AFR_SEC and TPS2 are illustrative redundant probes; AUX1/2 are radiator-outlet
//! and intercooler-outlet probes. Boost target is absolute, wastegate pressure gauge.

use super::Provenance;
use super::state::State;
use cogware_can::Gauge;

pub(super) enum Channel {
    Modelled(fn(&State) -> f64),
    Nominal(f64),
    Excluded,
}

impl Channel {
    pub(super) fn provenance(&self) -> Provenance {
        match self {
            Self::Modelled(_) => Provenance::Modelled,
            Self::Nominal(_) => Provenance::Nominal,
            Self::Excluded => Provenance::Excluded,
        }
    }

    pub(super) fn value(&self, state: &State) -> Option<f64> {
        match self {
            Self::Modelled(read) => Some(read(state)),
            Self::Nominal(value) => Some(*value),
            Self::Excluded => None,
        }
    }
}

pub(super) fn channel(gauge: Gauge) -> Channel {
    use Channel::{Excluded, Modelled as M, Nominal as N};
    use Gauge::*;
    match gauge {
        Indicators1 | Indicators2 | Masteralive => Excluded,
        // Opaque firmware flags are intentionally not an emulated ECU or heartbeat.
        StaStatus1 | StaEng | StaSpark | TestOutputs | NextError | StaStatus2 | EngProtectSta
        | StaStatus4 | StatusOutSta | NitroSta | SdSta | ErrorCount | LostSync | MissCount => {
            N(0.0)
        }
        LoopPs => N(1000.0),
        FreeMem => N(2048.0),
        EthanolPercent | FlexIgnCorrect | FlexBoostCorrect | WmiPw => N(0.0),
        FlexCorrect | FuelTempCorrect | StaLaunchCorrect => N(100.0),
        FuelPresHigh | IgnAngleTrail | InjDuty2 | VvtAngle2 | VvtTargetAngle2 | VvtDuty2
        | VvtExhAngle2 | VvtExhTarget2 | Egt5 | Egt6 | Egt7 | Egt8 | Knock5 | Knock6 | Knock7
        | Knock8 => N(0.0),
        KnockRetard | KnockCount | KnockCyl | IgnCut => N(0.0),
        GpsLock => N(2.0),
        GpsSats => N(11.0),
        SlipTarget => N(5.0),
        TractionLimit => N(100.0),
        RevLimit => N(6800.0),
        AccelRight => N(0.0),
        AccelUp => N(1.0),
        StaTime => M(|s| s.time.floor() % 256.0),
        Dwell => M(|s| s.dwell),
        Map => M(|s| s.map),
        Iat => M(|s| s.iat),
        Clnt => M(|s| s.coolant),
        BatCorrect => M(|s| 100.0 * 14.0 / s.battery),
        BatVol => M(|s| s.battery),
        AfrPri => M(|s| s.afr),
        EgoCorrect => M(|s| s.trim),
        IatCorrect => M(|s| 100.0 * 293.15 / (s.iat + 273.15)),
        WueCorrect => M(|s| s.warmup),
        Rpm => M(|s| s.rpm),
        AccelEnrich => M(|s| s.enrichment),
        GammeE => M(|s| s.warmup * s.enrichment / 100.0 * (1.0 + s.trim / 100.0)),
        Ve | Ve1 | Ve2 => M(|s| s.ve),
        AfrTarget => M(|s| s.afr_target),
        PulseWidth1 => M(|s| s.pulse),
        PulseWidth2 => M(|s| s.pulse * 0.992),
        PulseWidth3 => M(|s| s.pulse * 1.006),
        PulseWidth4 => M(|s| s.pulse * 1.002),
        TpsDot => M(|s| s.tps_dot),
        CurSparkAdvance | Advance1 => M(|s| s.spark),
        Advance2 => M(|s| s.spark - 0.2),
        Tps => M(|s| s.tps),
        BoostTarget => M(|s| s.boost_target),
        BoostPwm => M(|s| s.boost_pwm),
        RpmDot => M(|s| s.rpm_dot),
        IdleLoad => M(|s| s.idle_duty() * 2.55),
        AfrSec => M(|s| s.afr_secondary),
        Baro => M(|s| s.baro),
        TpsAdc => M(|s| s.tps * 2.55),
        FuelLoad | IgnLoad => M(|s| s.load() * 100.0),
        InjAngle => M(|s| 360.0 - s.rpm * 0.012),
        IdleDuty => M(|s| s.idle_duty()),
        ClIdleTarget => M(|s| s.idle_rpm()),
        MapDot => M(|s| s.map_dot),
        VvtAngle => M(|s| s.vvt),
        VvtTargetAngle => M(|s| s.vvt_target),
        VvtDuty => M(|s| (25.0 + (s.vvt_target - s.vvt) * 1.5).clamp(0.0, 100.0)),
        BaroCorrection => M(|s| 100.0 * 100.0 / s.baro),
        Ase => M(|s| s.afterstart),
        Vss | WheelSpeedUndriven => M(|s| s.speed),
        Gear => M(|s| f64::from(s.gear)),
        FuelPres => M(|s| 300.0 + s.map - s.baro),
        OilPres => M(|s| s.oil_pressure),
        FlexFuelTemp => M(|s| s.fuel_temp),
        OilTemp => M(|s| s.oil_temp),
        Egt1 => M(|s| s.egt),
        Egt2 => M(|s| s.egt + 8.0),
        Egt3 => M(|s| (s.egt - 6.0).max(20.0)),
        Egt4 => M(|s| s.egt + 4.0),
        FuelLevel => M(|s| s.fuel_litres / 55.0 * 100.0),
        CoolantPres => M(|s| ((s.coolant - 30.0) * 1.2 + s.rpm * 0.005).max(0.0)),
        Pedal => M(|s| s.pedal),
        TransTemp => M(|s| s.trans_temp),
        InjDuty => M(|s| s.injector_duty),
        WastegatePres => M(|s| (s.map - s.baro).max(0.0)),
        FuelVolume => M(|s| s.fuel_litres),
        Odometer => M(|s| 12400.0 + s.trip),
        TripDist => M(|s| s.trip),
        GpsSpeed => M(|s| s.gps_speed),
        KnockLevel => M(|s| s.knock()),
        Knock1 => M(|s| s.knock() * 0.96),
        Knock2 => M(|s| s.knock() * 1.04),
        Knock3 => M(|s| s.knock() * 0.98),
        Knock4 => M(|s| s.knock() * 1.02),
        Tps2 => M(|s| (s.tps + 0.2).min(100.0)),
        CoilDuty => M(|s| s.dwell * s.rpm / 1200.0),
        EcuTemp => M(|s| s.ecu_temp),
        AuxTemp1 => M(|s| (s.coolant - 6.0).max(20.0)),
        AuxTemp2 => M(|s| s.iat),
        DiffTemp => M(|s| s.diff_temp),
        CylAirmass => M(|s| s.air_mass),
        Airflow => M(|s| s.airflow),
        FuelUsed => M(|s| s.fuel_used),
        FuelFlow => M(|s| s.fuel_flow),
        FuelRate => M(|s| s.fuel_rate),
        EgoCorrect2 => M(|s| if s.fuel_cut { 0.0 } else { s.trim + 0.15 }),
        VvtExhAngle => M(|s| s.vvt_exhaust),
        VvtExhTarget => M(|s| s.vvt_exhaust_target),
        WheelSpeedDriven => M(|s| s.speed * (1.0 + s.slip / 100.0)),
        WheelSlip => M(|s| s.slip),
        FuelCut => M(|s| if s.fuel_cut { 100.0 } else { 0.0 }),
        IgnComp => M(|s| -(s.iat - 45.0).max(0.0) * 0.12),
        AccelFwd => M(|s| s.acceleration / 9.80665),
        TriggerCount => M(|s| (s.revolutions * 58.0).floor() % 65536.0),
        HomeCount => M(|s| (s.revolutions / 2.0).floor() % 65536.0),
        TrigSinceHome => M(|s| ((s.revolutions % 2.0) * 58.0).floor()),
    }
}
