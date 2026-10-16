ENTRY(_start)
OUTPUT_FORMAT(elf64-x86-64)

SECTIONS {
    . = 0x100000;

    _kernel_start = .;

    .text : ALIGN(4096) {
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .init_array : ALIGN(8) {
        __init_array_start = .;
        KEEP(*(.init_array))
        __init_array_end = .;
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    _kernel_end = .;
}
