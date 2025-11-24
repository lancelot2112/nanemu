use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use nanemu::soc::core::ExecutionHarness;
use nanemu::soc::isa::ast::MaskSelector;
use nanemu::soc::isa::machine::MachineDescription;
use nanemu::soc::isa::semantics::trace::{ExecutionTracer, HostOpKind, TraceEvent};

struct RecordingTracer {
    events: Rc<RefCell<Vec<TraceEvent>>>,
}

impl RecordingTracer {
    fn new(events: Rc<RefCell<Vec<TraceEvent>>>) -> Self {
        Self { events }
    }
}

impl ExecutionTracer for RecordingTracer {
    fn on_event(&mut self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
}

#[test]
fn tracer_captures_fetch_aliases_and_bit_widths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer");
    let coredef = root.join("tracer.coredef");
    let mut harness =
        ExecutionHarness::from_coredef("ppc-trace", &coredef, None).expect("construct harness");

    {
        let state = harness.state_mut();
        state
            .write_register("reg::r3", 0x7FFF_FFFF)
            .expect("seed r3");
        state.write_register("reg::r4", 1).expect("seed r4");
    }

    let rom = build_add_rom(harness.machine());
    let events = Rc::new(RefCell::new(Vec::new()));
    harness.enable_tracer(Box::new(RecordingTracer::new(events.clone())));

    harness
        .execute_block(0x8000_1000, &rom)
        .expect("execute rom for trace");

    let events = events.borrow();
    let trace_dump = format_trace(&events);
    assert!(
        !events.is_empty(),
        "expected tracer events\n{trace_dump}"
    );
    let fetch = events
        .iter()
        .find_map(|event| {
            if let TraceEvent::Fetch { mnemonic, detail, .. } = event {
                Some((mnemonic.clone(), detail.clone()))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("fetch event missing\n{trace_dump}"));
    assert_eq!(fetch.0, "add", "trace:\n{trace_dump}");
    assert!(
        fetch.1.contains("r5"),
        "fetch detail missing r5\n{trace_dump}"
    );

    assert_has_write(&events, "reg::CR0::NEG", 1, &trace_dump);
    assert_has_write(&events, "reg::XER::SO", 1, &trace_dump);
    assert_has_read(&events, "reg::XER::SO", 1, &trace_dump);

    assert!(
        events.iter().any(|event| matches!(
            event,
            TraceEvent::HostOp {
                op: HostOpKind::AddWithCarry, ..
            }
        )),
        "missing host op\n{trace_dump}"
    );
}

fn assert_has_write(events: &[TraceEvent], name: &str, width: u32, trace: &str) {
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::RegisterWrite { name: n, width: w, .. } if n == name && *w == width
    )), "missing write for {name}\n{trace}");
}

fn assert_has_read(events: &[TraceEvent], name: &str, width: u32, trace: &str) {
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::RegisterRead { name: n, width: w, .. } if n == name && *w == width
    )), "missing read for {name}\n{trace}");
}

fn build_add_rom(machine: &MachineDescription) -> Vec<u8> {
    let mut rom = Vec::new();
    rom.extend(encode_instruction(
        machine,
        "add",
        &[("RT", 5), ("RA", 3), ("RB", 4)],
    ));
    rom.extend(encode_instruction(
        machine,
        "add.",
        &[("RT", 6), ("RA", 5), ("RB", 4)],
    ));
    rom.extend(encode_instruction(
        machine,
        "addo",
        &[("RT", 7), ("RA", 3), ("RB", 3)],
    ));
    rom.extend(encode_instruction(
        machine,
        "addo.",
        &[("RT", 8), ("RA", 7), ("RB", 4)],
    ));
    rom
}

fn encode_instruction(
    machine: &MachineDescription,
    mnemonic: &str,
    operands: &[(&str, i64)],
) -> Vec<u8> {
    let instr = machine
        .instructions
        .iter()
        .find(|candidate| candidate.name == mnemonic)
        .unwrap_or_else(|| panic!("unknown instruction '{mnemonic}'"));
    let space = machine
        .spaces
        .get(&instr.space)
        .unwrap_or_else(|| panic!("instruction space '{}' missing", instr.space));
    let word_bits = space.word_bits().expect("logic space word size");
    assert_eq!(word_bits % 8, 0, "expected byte-aligned instruction");
    let word_bytes = (word_bits / 8) as usize;
    let mut bits = 0u64;

    if let Some(mask) = &instr.mask {
        for field in &mask.fields {
            let spec = match &field.selector {
                MaskSelector::Field(name) => {
                    let form_name = instr
                        .form
                        .as_ref()
                        .unwrap_or_else(|| panic!("instruction '{mnemonic}' missing form"));
                    let form = space
                        .forms
                        .get(form_name)
                        .unwrap_or_else(|| panic!("form '{form_name}' missing"));
                    form.subfield(name)
                        .unwrap_or_else(|| panic!("unknown field '{name}'"))
                        .spec
                        .clone()
                }
                MaskSelector::BitExpr(expr) => panic!(
                    "bit expression selector '{expr}' unsupported in trace tests"
                ),
            };
            bits = spec
                .write_bits(bits, field.value)
                .expect("apply mask constant");
        }
    }

    for &(operand, value) in operands {
        let form_name = instr
            .form
            .as_ref()
            .unwrap_or_else(|| panic!("instruction '{mnemonic}' missing form"));
        let form = space
            .forms
            .get(form_name)
            .unwrap_or_else(|| panic!("form '{form_name}' missing"));
        let field = form
            .subfield(operand)
            .unwrap_or_else(|| panic!("unknown operand '{operand}'"));
        bits = field
            .spec
            .write_bits(bits, value as u64)
            .expect("apply operand");
    }

    bits.to_be_bytes()[8 - word_bytes..].to_vec()
}

fn format_trace(events: &[TraceEvent]) -> String {
    let mut out = String::new();
    for event in events {
        let line = match event {
            TraceEvent::Fetch {
                address,
                opcode,
                mnemonic,
                detail,
            } => format!(
                "[Fetch] 0x{address:08X} 0x{opcode:08X} {mnemonic} {detail}",
                detail = detail.trim()
            ),
            TraceEvent::RegisterRead { name, value, width } => format!(
                "[ Read]   {name} -> {}",
                format_trace_value(*value, *width)
            ),
            TraceEvent::RegisterWrite { name, value, width } => format!(
                "[Write]   {name} <- {}",
                format_trace_value(*value, *width)
            ),
            TraceEvent::HostOp { op, args, result } => {
                if args.len() == 2 {
                    format!(
                        "[IntOp]   0x{lhs:016X} {op} 0x{rhs:016X} = 0x{result:016X}",
                        lhs = args[0],
                        rhs = args[1]
                    )
                } else {
                    format!("[IntOp]   {op:?} {:?} -> 0x{result:016X}", args)
                }
            }
        };
        let _ = writeln!(out, "{line}");
    }
    out
}

fn format_trace_value(value: i64, bits: u32) -> String {
    let width = std::cmp::max(1, ((bits as usize + 3) / 4) as usize);
    let masked = if bits == 0 {
        0
    } else if bits >= 64 {
        value as u64
    } else {
        let mask = (1u64 << bits) - 1;
        (value as u64) & mask
    };
    format!("0x{masked:0width$X}")
}
