use std::path::PathBuf;

use crate::loader::isa::IsaLoader;

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

    assert_eq!(listing[0].mnemonic, "addi");
    assert_eq!(listing[0].address, 0x1000);

    assert_eq!(listing[1].mnemonic, "se_b");
    assert_eq!(listing[1].address, 0x1004);
}
