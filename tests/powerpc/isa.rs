use std::path::PathBuf;

use crate::common;
use nanemu::loader::isa::IsaLoader;
use nanemu::soc::core::ExecutionHarness;
use nanemu::soc::isa::machine::{MachineDescription, SoftwareHost};
use nanemu::soc::isa::semantics::trace::PipelinePrinter;

struct InstructionCase {
    asm: Option<&'static str>,
    mnemonic: &'static str,
    operands: &'static [&'static str],
    display: Option<&'static str>,
    bytes: &'static [u8],
}

const ADDI_R0_R0_BYTES: [u8; 4] = 0x3800_0000u32.to_be_bytes();
const SE_B_0_BYTES: [u8; 2] = 0xE800u16.to_be_bytes();
const EVADDW_BYTES: [u8; 4] = [0x10, 0x64, 0x2A, 0x00];
const EVCMPEQ_BYTES: [u8; 4] = [0x10, 0x86, 0x3A, 0x34];
const EVSPLATI_BYTES: [u8; 4] = [0x11, 0x1F, 0x02, 0x29];
const EVLDD_BYTES: [u8; 4] = [0x11, 0x2A, 0x03, 0x01];
const EVSTDW_BYTES: [u8; 4] = [0x11, 0x6C, 0x0B, 0x23];

const VLE_CASES: [InstructionCase; 2] = [
    InstructionCase {
        asm: None,
        mnemonic: "addi",
        operands: &["r0", "r0", "0x0000"],
        display: Some("r0, r0, 0x0000"),
        bytes: &ADDI_R0_R0_BYTES,
    },
    InstructionCase {
        asm: None,
        mnemonic: "se_b",
        operands: &["0x000"],
        display: Some("0x000"),
        bytes: &SE_B_0_BYTES,
    },
];

const SPE_CASES: [InstructionCase; 5] = [
    InstructionCase {
        asm: Some("evaddw r3, r4, r5"),
        mnemonic: "evaddw",
        operands: &["r3", "r4", "r5"],
        display: Some("r3, r4, r5"),
        bytes: &EVADDW_BYTES,
    },
    InstructionCase {
        asm: Some("evcmpeq cr1, r6, r7"),
        mnemonic: "evcmpeq",
        operands: &["CR1", "r6", "r7"],
        display: Some("CR1, r6, r7"),
        bytes: &EVCMPEQ_BYTES,
    },
    InstructionCase {
        asm: Some("evsplati r8, -1"),
        mnemonic: "evsplati",
        operands: &["r8", "-1"],
        display: Some("r8, -1"),
        bytes: &EVSPLATI_BYTES,
    },
    InstructionCase {
        asm: Some("evldd r9, r10, 0"),
        mnemonic: "evldd",
        operands: &["r9", "r10", "0x00"],
        display: Some("r9, r10, 0x00"),
        bytes: &EVLDD_BYTES,
    },
    InstructionCase {
        asm: Some("evstdw r11, r12, 8"),
        mnemonic: "evstdw",
        operands: &["r11", "r12", "0x08"],
        display: Some("r11, r12, 0x08"),
        bytes: &EVSTDW_BYTES,
    },
];

#[test]
fn disassembles_powerpc_vle_stream() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut loader = IsaLoader::new();
    let machine = loader
        .load_machine(coredef)
        .expect("load powerpc + vle includes");

    let mut stream = Vec::new();
    for case in VLE_CASES.iter() {
        stream.extend_from_slice(case.bytes);
    }
    for case in SPE_CASES.iter() {
        stream.extend_from_slice(case.bytes);
    }

    let listing = machine.disassemble_from(&stream, 0x1000);
    assert_eq!(
        listing.len(),
        VLE_CASES.len() + SPE_CASES.len(),
        "expected VLE base stream plus SPE instructions"
    );

    if std::env::var_os("SHOW_DISASM").is_some() {
        eprintln!("PowerPC VLE listing:");
        for entry in &listing {
            if let Some(display) = &entry.display {
                eprintln!(
                    "  0x{addr:08X}: {mnemonic:<6} {display}",
                    addr = entry.address,
                    mnemonic = entry.mnemonic,
                    display = display,
                );
            } else {
                eprintln!(
                    "  0x{addr:08X}: {mnemonic:<6} {operands:?}",
                    addr = entry.address,
                    mnemonic = entry.mnemonic,
                    operands = entry.operands,
                );
            }
        }
    }

    let mut addr = 0x1000u64;
    for (idx, case) in VLE_CASES
        .iter()
        .chain(SPE_CASES.iter())
        .enumerate()
    {
        let entry = &listing[idx];
        assert_eq!(entry.mnemonic, case.mnemonic);
        assert_eq!(entry.address, addr);
        assert_eq!(entry.operands, case.operands);
        assert_eq!(entry.display.as_deref(), case.display);
        addr += case.bytes.len() as u64;
    }
}

#[test]
fn assembles_powerpc_spe_instructions() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut loader = IsaLoader::new();
    let machine = loader
        .load_machine(coredef)
        .expect("load powerpc + vle includes");

    for case in SPE_CASES.iter() {
        let asm = case
            .asm
            .expect("SPE cases should retain assembler inputs");
        let bytes = machine
            .assemble(asm)
            .unwrap_or_else(|err| panic!("failed to assemble '{asm}': {err}"));
        assert_eq!(
            bytes.as_slice(),
            case.bytes,
            "encoding mismatch for {asm}"
        );

        let listing = machine.disassemble_from(&bytes, 0x2000);
        assert_eq!(listing.len(), 1, "expected single-instruction listing");
        assert_eq!(listing[0].mnemonic, case.mnemonic);
        assert_eq!(listing[0].operands, case.operands);
        assert_eq!(listing[0].display.as_deref(), case.display);
    }
}

#[test]
fn executes_powerpc_add() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    harness.write("reg::r3", 0x7FFF_FFFF).expect("seed r3");
    harness.write("reg::r4", 1).expect("seed r4");

    let rom = assemble_block(harness.machine(), &["add r5, r3, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute add");
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].mnemonic, "add");

    let r5 = harness
        .state_mut()
        .read_register("reg::r5")
        .expect("read r5");
    assert_eq!(r5 as u32, 0x8000_0000);
}

#[test]
fn executes_powerpc_add_record_sets_cr0() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    harness.write("reg::r3", 0x7FFF_FFFF).expect("seed r3");
    harness.write("reg::r4", 1).expect("seed r4");

    let rom = assemble_block(harness.machine(), &["add r5, r3, r4", "add. r6, r5, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute add.");
    assert_eq!(executions.len(), 2);
    assert_eq!(executions[1].mnemonic, "add.");

    let r6 = harness.read("reg::r6").expect("read r6");
    assert_eq!(r6 as u32, 0x8000_0001);

    let (neg, pos, zero, raw) = get_cr0(&mut harness);
    let exp_neg = true;
    assert_eq!(
        neg, exp_neg,
        "CR0::NEG should be {exp_neg}, got {neg} (raw=0x{raw:X})"
    );
    let exp_pos = false;
    assert_eq!(
        pos, exp_pos,
        "CR0::POS should be {exp_pos}, got {pos} (raw=0x{raw:X})"
    );
    let exp_zero = false;
    assert_eq!(
        zero, exp_zero,
        "CR0::ZERO should be {exp_zero}, got {zero} (raw=0x{raw:X})"
    );
}

fn get_cr0(harness: &mut ExecutionHarness<SoftwareHost>) -> (bool, bool, bool, u64) {
    let raw = harness.read("reg::CR0").expect("CR0 raw");
    let neg = harness.read("reg::CR0::NEG").expect("CR0::NEG") == 1;
    let pos = harness.read("reg::CR0::POS").expect("CR0::POS") == 1;
    let zero = harness.read("reg::CR0::ZERO").expect("CR0::ZERO") == 1;
    (neg, pos, zero, raw)
}

#[test]
fn executes_powerpc_add_with_overflow() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    harness.write("reg::r3", 0xFFFF_FFFF).expect("seed r3");
    harness.write("reg::r4", 1).expect("seed r4");
    harness.write("reg::XER", 0).expect("clear XER");

    let rom = assemble_block(harness.machine(), &["addo r7, r3, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute addo");
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].mnemonic, "addo");

    let r7 = harness.read("reg::r7").expect("read r7");
    assert_eq!(r7 as u32, 0);

    check_summary_overflow(&mut harness, true, true, false);
}

#[test]
fn executes_powerpc_add_with_overflow_and_record() {
    let _lock = common::serial();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    harness.write("reg::r3", 0xFFFF_FFFF).expect("seed r3");
    harness.write("reg::r4", 1).expect("seed r4");
    harness.write("reg::XER", 0).expect("clear XER");

    let rom = assemble_block(harness.machine(), &["addo. r8, r3, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute addo.");
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].mnemonic, "addo.");

    let r8 = harness.read("reg::r8").expect("read r8");
    assert_eq!(r8 as u32, 0);

    check_summary_overflow(&mut harness, true, true, true);
    let (neg, pos, zero, raw) = get_cr0(&mut harness);
    let exp_neg = false;
    assert_eq!(
        neg, exp_neg,
        "CR0::NEG should be {exp_neg}, got {neg} (raw=0x{raw:X})"
    );
    let exp_pos = false;
    assert_eq!(
        pos, exp_pos,
        "CR0::POS should be {exp_pos}, got {pos} (raw=0x{raw:X})"
    );
    let exp_zero = true;
    assert_eq!(
        zero, exp_zero,
        "CR0::ZERO should be {exp_zero}, got {zero} (raw=0x{raw:X})"
    );
}

fn check_summary_overflow(
    harness: &mut ExecutionHarness<SoftwareHost>,
    exp_xer_ov: bool,
    exp_xer_so: bool,
    exp_cr_so: bool,
) {
    let xer_ov = harness.read("reg::XER::OV").expect("read XER::OV") == 1;
    let cr_so = harness.read("reg::CR0::SO").expect("read CR0::SO") == 1;
    let cr_raw = harness.read("reg::CR0").expect("read CR0");
    let xer_so = harness.read("reg::XER::SO").expect("read XER::SO") == 1;
    assert_eq!(
        xer_ov, exp_xer_ov,
        "overflow should be {exp_xer_ov}, (ov={xer_ov}, cr_so={cr_so},  xer_so={xer_so})"
    );
    assert_eq!(
        xer_so, exp_xer_so,
        "summary overflow should be {exp_xer_so}, (ov={xer_ov}, cr_so={cr_so}, xer_so={xer_so})"
    );
    assert_eq!(
        cr_so, exp_cr_so,
        "CR summary overflow should be {exp_cr_so}, (ov={xer_ov}, cr_so={cr_so}, xer_so={xer_so}, cr_raw=0x{cr_raw:X})"
    );
}

fn enable_trace_if_requested(harness: &mut ExecutionHarness<SoftwareHost>) {
    if std::env::var_os("TRACE_PIPELINE").is_some() {
        harness.enable_tracer(Box::new(PipelinePrinter::stdout()));
    }
}

fn build_powerpc_harness(coredef: &PathBuf) -> ExecutionHarness<SoftwareHost> {
    let mut harness =
        ExecutionHarness::from_coredef("ppc-e200", coredef, None).expect("construct harness");
    enable_trace_if_requested(&mut harness);
    harness
}

fn assemble_block(machine: &MachineDescription, lines: &[&str]) -> Vec<u8> {
    let mut rom = Vec::new();
    for line in lines {
        let bytes = machine.assemble(line).unwrap_or_else(|err| {
            panic!("failed to assemble '{line}': {err}");
        });
        rom.extend(bytes);
    }
    rom
}
