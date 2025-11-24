use std::path::PathBuf;

use nanemu::loader::isa::IsaLoader;
use nanemu::soc::core::ExecutionHarness;
use nanemu::soc::isa::machine::{MachineDescription, SoftwareHost};
use nanemu::soc::isa::semantics::trace::PipelinePrinter;

#[test]
fn disassembles_powerpc_vle_stream() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut loader = IsaLoader::new();
    let machine = loader
        .load_machine(coredef)
        .expect("load powerpc + vle includes");

    let addi = 0x3800_0000u32.to_be_bytes();
    let se_b = 0xE800u16.to_be_bytes();

    let mut stream = Vec::new();
    stream.extend_from_slice(&addi);
    stream.extend_from_slice(&se_b);

    let listing = machine.disassemble_from(&stream, 0x1000);
    assert_eq!(listing.len(), 2, "expected 32-bit + 16-bit instructions");

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

    assert_eq!(listing[0].mnemonic, "addi");
    assert_eq!(listing[0].address, 0x1000);
    assert_eq!(
        listing[0].operands,
        vec!["r0", "r0", "0x0000"],
        "disp formatting should rename registers"
    );
    assert_eq!(listing[0].display.as_deref(), Some("r0, r0, 0x0000"));

    assert_eq!(listing[1].mnemonic, "se_b");
    assert_eq!(listing[1].address, 0x1004);
    assert_eq!(listing[1].display.as_deref(), Some("0x000"));
}

#[test]
fn executes_powerpc_add() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    seed_base_gprs(&mut harness);

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
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    seed_base_gprs(&mut harness);

    let rom = assemble_block(harness.machine(), &["add r5, r3, r4", "add. r6, r5, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute add.");
    assert_eq!(executions.len(), 2);
    assert_eq!(executions[1].mnemonic, "add.");

    let r6 = harness
        .state_mut()
        .read_register("reg::r6")
        .expect("read r6");
    assert_eq!(r6 as u32, 0x8000_0001);

    let neg = harness
        .read_register_value("reg", "CR0", Some("NEG"), None)
        .expect("CR0::NEG")
        .as_int()
        .expect("neg int");
    let pos = harness
        .read_register_value("reg", "CR0", Some("POS"), None)
        .expect("CR0::POS")
        .as_int()
        .expect("pos int");
    let zero = harness
        .read_register_value("reg", "CR0", Some("ZERO"), None)
        .expect("CR0::ZERO")
        .as_int()
        .expect("zero int");
    assert_eq!(neg, 0);
    assert_eq!(pos, 0);
    assert_eq!(zero, 0);
}

#[test]
fn executes_powerpc_add_with_overflow() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    seed_overflow_gprs(&mut harness);
    harness
        .write_register_value("reg", "XER", None, None, 0)
        .expect("clear XER");

    let rom = assemble_block(harness.machine(), &["addo r7, r3, r3"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute addo");
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].mnemonic, "addo");

    let r7 = harness
        .state_mut()
        .read_register("reg::r7")
        .expect("read r7");
    assert_eq!(r7 as u32, 0xFFFF_FFFE);

    let xer_ov = harness
        .read_register_value("reg", "XER", Some("OV"), None)
        .expect("XER::OV")
        .as_int()
        .expect("ov int");
    let xer_so = harness
        .read_register_value("reg", "XER", Some("SO"), None)
        .expect("XER::SO")
        .as_int()
        .expect("so int");
    assert_eq!(xer_ov, 0, "addo should leave overflow clear");
    assert_eq!(xer_so, 0, "addo should leave summary overflow clear");
}

#[test]
fn executes_powerpc_add_with_overflow_and_record() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("defs/powerpc");
    let coredef = root.join("e200.coredef");
    let mut harness = build_powerpc_harness(&coredef);
    seed_overflow_gprs(&mut harness);
    harness
        .write_register_value("reg", "XER", None, None, 0)
        .expect("clear XER");

    let rom = assemble_block(harness.machine(), &["addo r7, r3, r3", "addo. r8, r7, r4"]);
    let executions = harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute addo.");
    assert_eq!(executions.len(), 2);
    assert_eq!(executions[1].mnemonic, "addo.");

    let r8 = harness
        .state_mut()
        .read_register("reg::r8")
        .expect("read r8");
    assert_eq!(r8 as u32, 0xFFFF_FFFF);

    let neg = harness
        .read_register_value("reg", "CR0", Some("NEG"), None)
        .expect("CR0::NEG")
        .as_int()
        .expect("neg int");
    let pos = harness
        .read_register_value("reg", "CR0", Some("POS"), None)
        .expect("CR0::POS")
        .as_int()
        .expect("pos int");
    let zero = harness
        .read_register_value("reg", "CR0", Some("ZERO"), None)
        .expect("CR0::ZERO")
        .as_int()
        .expect("zero int");
    assert_eq!(neg, 0);
    assert_eq!(pos, 0);
    assert_eq!(zero, 0);

    let cr_so = harness
        .read_register_value("reg", "CR0", Some("SO"), None)
        .expect("CR0::SO")
        .as_int()
        .expect("so int");
    let xer_so = harness
        .read_register_value("reg", "XER", Some("SO"), None)
        .expect("XER::SO")
        .as_int()
        .expect("so int");
    assert_eq!(cr_so, xer_so, "addo. should mirror XER::SO into CR0");
    assert_eq!(cr_so, 0, "addo. should leave summary overflow clear");
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

fn seed_base_gprs(harness: &mut ExecutionHarness<SoftwareHost>) {
    let state = harness.state_mut();
    state
        .write_register("reg::r3", 0x7FFF_FFFF)
        .expect("seed r3");
    state.write_register("reg::r4", 1).expect("seed r4");
}

fn seed_overflow_gprs(harness: &mut ExecutionHarness<SoftwareHost>) {
    let state = harness.state_mut();
    state
        .write_register("reg::r3", 0x7FFF_FFFF)
        .expect("seed r3");
    state.write_register("reg::r4", 1).expect("seed r4");
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
