#![allow(unused)]
#![no_std]
#![no_main]

mod gauge;
pub use gauge::*;
use mcp2515::frame::CanFrame;

pub fn cli_wri(frame: CanFrame, id: u16) {
    Gauge::from_repr(id)
        .expect("bad ID fucking idiot")
        .set_from_frame(frame);
}

pub fn server_framegen(id: u16) -> Option<CanFrame> {
    Gauge::from_repr(id)?.to_frame()
}

pub fn speeduino_n_writer(buf: [u8; 126]) {
    STA_TIME.set(buf[3] as _);
    STA_STATUS1.set(buf[4] as _);
    STA_ENG.set(buf[5] as _);
    DWELL.set(buf[6] as _);
    MAP.set_from_bytes(&buf[7..=8]);
    IAT.set(buf[9] as _);
    CLNT.set(buf[10] as _);
    BAT_CORRECT.set(buf[11] as _);
    BAT_VOL.set(buf[12] as _);
    AFR_PRI.set(buf[13] as _);
    EGO_CORRECT.set(buf[14] as _);
    IAT_CORRECT.set(buf[15] as _);
    WUE_CORRECT.set(buf[16] as _);
    RPM.set_from_bytes(&buf[17..=18]);
    ACCEL_ENRICH.set(buf[19] as _);
    GAMME_E.set(buf[20] as _);
    VE.set(buf[21] as _);
    AFR_TARGET.set(buf[22] as _);
    PULSE_WIDTH1.set_from_bytes(&buf[23..=24]);
    TPS_DOT.set(buf[25] as _);
    CUR_SPARK_ADVANCE.set(buf[26] as _);
    TPS.set(buf[27] as _);
    LOOP_PS.set_from_bytes(&buf[28..=29]);
    FREE_MEM.set_from_bytes(&buf[30..=31]);
    BOOST_TARGET.set(buf[32] as _);
    BOOST_PWM.set(buf[33] as _);
    STA_SPARK.set(buf[34] as _);
    RPM_DOT.set_from_bytes(&buf[35..=36]);
    ETHANOL_PERCENT.set(buf[37] as _);
    FLEX_CORRECT.set(buf[38] as _);
    FLEX_IGN_CORRECT.set(buf[39] as _);
    IDLE_LOAD.set(buf[40] as _);
    TEST_OUTPUTS.set(buf[41] as _);
    AFR_SEC.set(buf[42] as _);
    BARO.set(buf[43] as _);
    TPS_ADC.set(buf[76] as _);
    NEXT_ERROR.set(buf[77] as _);
    STA_LAUNCH_CORRECT.set(buf[78] as _);
    PULSE_WIDTH2.set_from_bytes(&buf[79..=80]);
    PULSE_WIDTH3.set_from_bytes(&buf[81..=82]);
    PULSE_WIDTH4.set_from_bytes(&buf[83..=84]);
    STA_STATUS2.set(buf[85] as _);
    ENG_PROTECT_STA.set(buf[86] as _);
    FUEL_LOAD.set_from_bytes(&buf[87..=88]);
    IGN_LOAD.set_from_bytes(&buf[89..=90]);
    INJ_ANGLE.set_from_bytes(&buf[91..=92]);
    IDLE_DUTY.set(buf[93] as _);
    CL_IDLE_TARGET.set(buf[94] as _);
    MAP_DOT.set(buf[95] as _);
    VVT_ANGLE.set(buf[96] as _);
    VVT_TARGET_ANGLE.set(buf[97] as _);
    VVT_DUTY.set(buf[98] as _);
    FLEX_BOOST_CORRECT.set_from_bytes(&buf[99..=100]);
    BARO_CORRECTION.set(buf[101] as _);
    ASE.set(buf[102] as _);
    VSS.set_from_bytes(&buf[103..=104]);
    GEAR.set(buf[105] as _);
    FUEL_PRES.set(buf[106] as _);
    OIL_PRES.set(buf[107] as _);
    WMI_PW.set(buf[108] as _);
    STA_STATUS4.set(buf[109] as _);
    VVT_ANGLE2.set(buf[110] as _);
    VVT_TARGET_ANGLE2.set(buf[111] as _);
    VVT_DUTY2.set(buf[112] as _);
    STATUS_OUT_STA.set(buf[113] as _);
    FLEX_FUEL_TEMP.set(buf[114] as _);
    FUEL_TEMP_CORRECT.set(buf[115] as _);
    VE1.set(buf[116] as _);
    VE2.set(buf[117] as _);
    ADVANCE1.set(buf[118] as _);
    ADVANCE2.set(buf[119] as _);
    NITRO_STA.set(buf[120] as _);
    SD_STA.set(buf[121] as _);
}

#[allow(non_snake_case)]
pub fn speeduino_A_writer(buf: [u8; 126]) {
    STA_TIME.set(buf[2] as _);
    STA_STATUS1.set(buf[3] as _);
    STA_ENG.set(buf[4] as _);
    DWELL.set(buf[5] as _);
    MAP.set_from_bytes(&buf[6..=7]);
    IAT.set(buf[8] as _);
    CLNT.set(buf[9] as _);
    BAT_CORRECT.set(buf[10] as _);
    BAT_VOL.set(buf[11] as _);
    AFR_PRI.set(buf[12] as _);
    EGO_CORRECT.set(buf[13] as _);
    IAT_CORRECT.set(buf[14] as _);
    WUE_CORRECT.set(buf[15] as _);
    RPM.set_from_bytes(&buf[16..=17]);
    ACCEL_ENRICH.set(buf[18] as _);
    GAMME_E.set(buf[19] as _);
    VE.set(buf[20] as _);
    AFR_TARGET.set(buf[21] as _);
    PULSE_WIDTH1.set_from_bytes(&buf[22..=23]);
    TPS_DOT.set(buf[24] as _);
    CUR_SPARK_ADVANCE.set(buf[25] as _);
    TPS.set(buf[26] as _);
    LOOP_PS.set_from_bytes(&buf[27..=28]);
    FREE_MEM.set_from_bytes(&buf[29..=30]);
    BOOST_TARGET.set(buf[31] as _);
    BOOST_PWM.set(buf[32] as _);
    STA_SPARK.set(buf[33] as _);
    RPM_DOT.set_from_bytes(&buf[34..=35]);
    ETHANOL_PERCENT.set(buf[36] as _);
    FLEX_CORRECT.set(buf[37] as _);
    FLEX_IGN_CORRECT.set(buf[38] as _);
    IDLE_LOAD.set(buf[39] as _);
    TEST_OUTPUTS.set(buf[40] as _);
    AFR_SEC.set(buf[41] as _);
    BARO.set(buf[42] as _);
    TPS_ADC.set(buf[75] as _);
}
