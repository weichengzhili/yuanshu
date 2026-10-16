ENTRY(_start)
OUTPUT_FORMAT(elf64-x86-64)

SECTIONS {
    . = 0x400000;

    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }

    .init_array : ALIGN(8) {
        __init_array_start = .;
        KEEP(*(.init_array))
        __init_array_end = .;
    }

    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
}
