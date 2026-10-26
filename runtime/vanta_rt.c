#include <stdint.h>
#include <unistd.h>
#include <errno.h>

static void safe_write_all(int fd, const char *buf, size_t count) {
    size_t written = 0;
    while (written < count) {
        ssize_t res = write(fd, buf + written, count - written);
        if (res > 0) {
            written += (size_t)res;
        } else if (res < 0) {
            if (errno == EINTR) {
                continue;
            }
            // Non-recoverable write error - stop writing
            break;
        } else {
            // EOF or zero write
            break;
        }
    }
}

void print_str(const char *ptr, uint64_t len) {
    if (len == 0 || ptr == NULL) {
        return;
    }
    safe_write_all(1, ptr, (size_t)len);
}

void print_i64(int64_t val) {
    char buf[32];
    char *p = buf + sizeof(buf);
    uint64_t uval;
    int is_neg = 0;

    if (val < 0) {
        is_neg = 1;
        // Avoid signed overflow on i64::MIN (-9223372036854775808)
        uval = (uint64_t)(-(val + 1)) + 1;
    } else {
        uval = (uint64_t)val;
    }

    if (uval == 0) {
        *(--p) = '0';
    } else {
        while (uval > 0) {
            *(--p) = (char)('0' + (uval % 10));
            uval /= 10;
        }
    }

    if (is_neg) {
        *(--p) = '-';
    }

    size_t len = (size_t)((buf + sizeof(buf)) - p);
    safe_write_all(1, p, len);
}
