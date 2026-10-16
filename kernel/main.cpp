#include <types.h>
#include <config.h>

extern "C" void kernel_main() {
    for (;;) asm volatile("hlt");
}
