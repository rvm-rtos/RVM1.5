# Boot application processors.

# Each non-boot CPU ("AP") is started up in response to a STARTUP
# IPI from the boot CPU.  Section B.4.2 of the Multi-Processor
# Specification says that the AP will start in real mode with CS:IP
# set to XY00:0000, where XY is an 8-bit value sent with the
# STARTUP. Thus this code must start at a 4096-byte boundary.
#
# Because this code sets DS to zero, it must sit
# at an address in the low 2^16 bytes.

.equ pa_start32, start32 - ap_start + 0x6000
.equ pa_start64, start64 - ap_start + 0x6000
.equ pa_gdt, ap_gdt - ap_start + 0x6000
.equ pa_gdt_desc, ap_gdt_desc - ap_start + 0x6000
.equ pa_tmp_pgd, 0x7000

.equ cr3_ptr, 0x6ff8
.equ entry_ptr, 0x6ff0
.equ stack_ptr, 0x6fe8

.global ap_start
.global ap_end

# 0x6000
.section .text
.code16
.p2align 12
ap_start:
    cli
    wbinvd

    xor     ax, ax
    mov     ds, ax
    mov     es, ax
    mov     ss, ax

    # load the 64-bit GDT
    lgdt    [pa_gdt_desc]

    # switch to protected-mode
    mov     eax, cr0
    or      eax, (1 << 0)
    mov     cr0, eax

    # ljmp     0x8, start32
    .byte 0xea
    .short pa_start32
    .short 0x8

.code32
.balign 4
start32:
    mov     ax, 0x18
    mov     ds, ax

    # set PAE, PGE, OSFXSR, OSXMMEXCPT bit
    mov     eax, cr4
    or      eax, (1 << 5) | (1 << 7) | (1 << 9) | (1 << 10)
    mov     cr4, eax

    # load the temporary cr3 to fit in 4GB address space
    mov     eax, pa_tmp_pgd
    mov     cr3, eax

    # set LME, NXE bit in IA32_EFER
    mov     ecx, 0xC0000080
    rdmsr
    and     eax, ~(1 << 10) # clear LMA
    or      eax, (1 << 8) | (1 << 11)
    wrmsr

    # set protected mode, write protect, paging bit
    mov     eax, cr0
    or      eax, (1 << 0) | (1 << 16) | (1 << 31)
    mov     cr0, eax

    # ljmp     0x10, start64
    .byte 0xea
    .long pa_start64
    .short 0x10

.code64
.balign 4
start64:
    mov     ax, 0x0
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     fs, ax
    mov     gs, ax

    # load the real cr3
    mov     rax, [cr3_ptr]
    mov     cr3, rax

    # set stack and jump to entry
    mov     rsp, [stack_ptr]
    mov     rax, [entry_ptr]
    call    rax

spin_hlt:
    hlt
    jmp     spin_hlt

.balign 16
ap_gdt:
    .quad 0x0000000000000000            # Null Descriptor - should be present.
    .quad 0x00cf9b000000ffff            # 32-bit code descriptor (exec/read).
    .quad 0x00af9b000000ffff            # 64-bit code descriptor (exec/read).
    .quad 0x00cf93000000ffff            # 64-bit data descriptor (read/write).

ap_gdt_desc:
    .short ap_gdt_desc - ap_gdt - 1     # 16-bit Size (Limit) of GDT.
    .long pa_gdt                        # 32-bit Base Address of GDT. (CPU will zero extend to 64-bit)

# 0x7000
.p2align 12
tmp_pgd:
    .quad 0x8000 | 0x3      # PRESENT | WRITABLE | paddr(0x8000)
    .zero 8 * 511

# 0x8000
tmp_pud:
    .quad 0x0000 | 0x83     # PRESENT | WRITABLE | HUGE_PAGE | paddr(0x0)
    .zero 8 * 511

# 0x9000
ap_end:
