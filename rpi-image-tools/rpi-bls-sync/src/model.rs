//! RPi model detection → target kernel filename on the EFI partition.
//! Ground truth: rpi-bls-sync.sh lines 19-27.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelName {
    Rpi5,
    Rpi4,
}

impl KernelName {
    pub fn efi_filename(self) -> &'static str {
        match self {
            KernelName::Rpi5 => "kernel_2712.img",
            KernelName::Rpi4 => "kernel8.img",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnsupportedModel(pub String);

pub fn parse_model(raw: &str) -> Result<KernelName, UnsupportedModel> {
    let model = raw.replace('\0', "");
    // Order matters: "Raspberry Pi 5" MUST be checked before "Raspberry Pi
    // 4" (rpi-bls-sync.sh lines 21-22 case-arm order) — a case statement
    // takes the first matching arm.
    if model.contains("Raspberry Pi 5") {
        Ok(KernelName::Rpi5)
    } else if model.contains("Raspberry Pi 4") {
        Ok(KernelName::Rpi4)
    } else {
        Err(UnsupportedModel(model))
    }
}
