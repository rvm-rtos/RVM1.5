use core::alloc::Layout;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let cpu_data = crate::percpu::current();
    error!("\n{}\nCurrent Cpu: {:#x?}", info, cpu_data);
    loop {}
}

#[lang = "oom"]
fn oom(_: Layout) -> ! {
    panic!("out of memory");
}
