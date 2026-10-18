#pragma once
using   u8 =unsigned char;
using   u16 =unsigned short;
using   u32 =unsigned int;
using   u64 =unsigned long long;
// 编译时级别：低于此级别整条调用被编译器裁掉

#ifndef XSO_LOG_LEVEL
#define XSO_LOG_LEVEL  2     // 0=TRACE 1=DEBUG 2=INFO 3=WARN 4=ERROR
#endif
enum  LogLevel: int {
    LOG_TRACE = 0,
    LOG_DEBUG = 1,
    LOG_INFO  = 2,
    LOG_WARN  = 3,
    LOG_ERROR = 4,
    LOG_PANIC = 5,
};
 void kernel_log_init();
 void  kernel_log_write(LogLevel level, const  char  * tag, const  char  *  fmt, ...);
#define  LOG_T ( tag, fmt , ...) \
    do{ if(LOG_TRACE >XSO_LOG_LEVEL) kernel_log_write(LOG_TRACE,tag, fmt,  ##__VA_ARGS__);  }while (0)
#define LOG_D( tag, fmt,...)\
  do { if (LOG_DEBUG >XSO_LOG_LEVEL) kernel_log_write(LOG_DEBUG,tag,fmt, ##__VA_ARGS__); } while (0)
#define LOG_I( TAG, FMT,...) \
   do{ if(LOG_INFO >XSO_LOG_LEVEL ) kernel_log_write(LOG_INFO,tag, fmt, ##__VA_ARGS__);  } while(0)
#define LOG_W(tag, fmt,...) \
    do{ if (LOG_WARN >XSO_LOG_LEVEL) kernel_log_write(LOG_WARN,tag,fmt, ##__VA_ARGS__);} while(0)

#define  LOG_E( tag, fmt,...)\
    do{ if(LOG_ERROR >XSO_LOG_LEVEL) kernel_log_write(LOG_ERROR, tag, fmt, ##__VA_ARGS__);}while(0)

#define LOG_P(tag, fmt, ...) \
   do{ if(LOG_PANIC > XSO_LOG_LEVEL) kernel_log_write(LOG_PANIC, tag, fmt,##__VA_ARGS__ );  } while(0)

