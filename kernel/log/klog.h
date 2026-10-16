//
// Created by User on 2026/10/16.
//
#pragma once

#include<type.h>


enum  LogLevel: int {
    LOG_TRACE = 0,
    LOG_DEBUG = 1,
    LOG_INFO  = 2,
    LOG_WARN  = 3,
    LOG_ERROR = 4,
    LOG_PANIC = 5,
};
namespace kernel {
    class KLog {
    public:
        static void Log(const char *fmt, ...);
    };
}
