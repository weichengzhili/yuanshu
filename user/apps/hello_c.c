/* hello_c.c —— 用户态示例 */
static long sys_write(int fd, const void* buf, unsigned long n) {
    long ret;
    __asm__ __volatile__("syscall"
        : "=a"(ret) : "a"(1), "D"(fd), "S"(buf), "d"(n)
        : "rcx","r11","memory");
    return ret;
}
static long sys_exit(int code) {
    long ret;
    __asm__ __volatile__("syscall"
        : "=a"(ret) : "a"(60), "D"(code)
        : "rcx","r11","memory");
    return ret;
}
void _start() {
    const char msg[] = "Hello from user space (C)!\n";
    sys_write(1, msg, sizeof(msg)-1);
    sys_exit(0);
}
