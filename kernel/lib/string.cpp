#include <types.h>

extern "C" void* kmemcpy(void* dst, const void* src, usize n) {
    auto* d = (u8*)dst; auto* s = (const u8*)src;
    while (n--) *d++ = *s++;
    return dst;
}
