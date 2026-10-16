/* hello_cpp.cpp —— C++ 用户态示例 */
extern "C" long __sys_write(int, const void*, unsigned long);
extern "C" void __sys_exit(int) __attribute__((noreturn));

extern "C" void _start() {
    const char msg[] = "Hello from user space (C++)!\n";
    __sys_write(1, msg, sizeof(msg)-1);
    __sys_exit(0);
}
