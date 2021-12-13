use core::fmt::{Debug, Formatter, Result};

use crate::consts::{HV_HEADER_PTR, PER_CPU_SIZE};

const HEADER_SIGNATURE: [u8; 8] = *b"RVMIMAGE";

#[repr(C)]
pub struct HvHeader {
    pub signature: [u8; 8],
    pub core_size: usize,
    pub percpu_size: usize,
    pub entry: usize,
    pub max_cpus: u32,
    pub rt_cpus: u32,
}

impl HvHeader {
    pub fn get<'a>() -> &'a Self {
        unsafe { &*HV_HEADER_PTR }
    }

    pub fn vm_cpus(&self) -> u32 {
        if self.rt_cpus < self.max_cpus {
            self.max_cpus - self.rt_cpus
        } else {
            warn!(
                "Invalid HvHeader: rt_cpus ({}) >= max_cpus ({})",
                self.rt_cpus, self.max_cpus
            );
            self.max_cpus
        }
    }
}

#[repr(C)]
struct HvHeaderStuff {
    signature: [u8; 8],
    core_size: unsafe extern "C" fn(),
    percpu_size: usize,
    entry: unsafe extern "C" fn(),
    max_cpus: u32,
    rt_cpus: u32,
}

extern "C" {
    fn __entry_offset();
    fn __core_size();
}

#[used]
#[link_section = ".header"]
static HEADER_STUFF: HvHeaderStuff = HvHeaderStuff {
    signature: HEADER_SIGNATURE,
    core_size: __core_size,
    percpu_size: PER_CPU_SIZE,
    entry: __entry_offset,
    max_cpus: 0,
    rt_cpus: 0,
};

impl Debug for HvHeader {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_struct("HvHeader")
            .field("signature", &core::str::from_utf8(&self.signature))
            .field("core_size", &self.core_size)
            .field("percpu_size", &self.percpu_size)
            .field("entry", &self.entry)
            .field("max_cpus", &self.max_cpus)
            .field("rt_cpus", &self.rt_cpus)
            .field("vm_cpus", &self.vm_cpus())
            .finish()
    }
}
