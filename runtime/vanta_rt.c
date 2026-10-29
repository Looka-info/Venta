#include <stdint.h>
#include <unistd.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/types.h>

static int g_argc = 0;
static char **g_argv = NULL;

__attribute__((constructor))
void vanta_rt_ctor(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

void vanta_rt_init(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

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

static void write_i64_to_fd(int fd, int64_t val) {
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
    safe_write_all(fd, p, len);
}

void print_str(const char *ptr, uint64_t len) {
    if (len == 0 || ptr == NULL) {
        return;
    }
    safe_write_all(1, ptr, (size_t)len);
}

void print_i64(int64_t val) {
    write_i64_to_fd(1, val);
}

void vanta_panic(const char *ptr, uint64_t len) {
    const char prefix[] = "Vanta panic: ";
    safe_write_all(2, prefix, sizeof(prefix) - 1);
    if (ptr != NULL && len > 0) {
        safe_write_all(2, ptr, (size_t)len);
    }
    const char newline[] = "\n";
    safe_write_all(2, newline, sizeof(newline) - 1);
    _exit(101);
}

void vanta_bounds_check_fail(int64_t index, int64_t length) {
    const char msg1[] = "Vanta panic: index out of bounds (index ";
    const char msg2[] = ", length ";
    const char msg3[] = ")\n";

    safe_write_all(2, msg1, sizeof(msg1) - 1);
    write_i64_to_fd(2, index);
    safe_write_all(2, msg2, sizeof(msg2) - 1);
    write_i64_to_fd(2, length);
    safe_write_all(2, msg3, sizeof(msg3) - 1);
    _exit(101);
}

static int64_t g_vanta_alloc_count = 0;
static int64_t g_vanta_free_count = 0;

void vanta_free(void *ptr);

void *vanta_alloc(uint64_t size) {
    if (size == 0) {
        return NULL;
    }
    // Guard against overflow: requests exceeding signed 63-bit range
    if (size > (uint64_t)0x7fffffffffffffffULL) {
        vanta_panic("allocation size overflow", 24);
    }
    void *ptr = malloc((size_t)size);
    if (ptr == NULL) {
        vanta_panic("out of memory", 13);
    }
    g_vanta_alloc_count++;
    return ptr;
}

void *vanta_realloc(void *ptr, uint64_t new_size) {
    if (ptr == NULL) {
        return vanta_alloc(new_size);
    }
    if (new_size == 0) {
        vanta_free(ptr);
        return NULL;
    }
    if (new_size > (uint64_t)0x7fffffffffffffffULL) {
        vanta_panic("allocation size overflow", 24);
    }
    void *new_ptr = realloc(ptr, (size_t)new_size);
    if (new_ptr == NULL) {
        vanta_panic("out of memory", 13);
    }
    return new_ptr;
}

void vanta_free(void *ptr) {
    if (ptr == NULL) {
        return;
    }
    g_vanta_free_count++;
    free(ptr);
}

int64_t vanta_get_alloc_count(void) {
    return g_vanta_alloc_count;
}

int64_t vanta_get_free_count(void) {
    return g_vanta_free_count;
}

int64_t vanta_active_allocs(void) {
    return g_vanta_alloc_count - g_vanta_free_count;
}

typedef struct {
    void *data;
    int64_t len;
    int64_t cap;
} VantaVec;

void vanta_vec_init(VantaVec *vec) {
    if (vec != NULL) {
        vec->data = NULL;
        vec->len = 0;
        vec->cap = 0;
    }
}

void vanta_vec_push(VantaVec *vec, int64_t val, int64_t elem_size) {
    if (vec == NULL) return;
    if (elem_size <= 0) elem_size = 8;
    if (vec->len >= vec->cap) {
        int64_t new_cap = (vec->cap == 0) ? 4 : (vec->cap * 2);
        int64_t new_size = new_cap * elem_size;
        vec->data = vanta_realloc(vec->data, (uint64_t)new_size);
        vec->cap = new_cap;
    }
    const void *src = (elem_size > 8) ? (const void *)val : (const void *)&val;
    memcpy((char *)vec->data + (vec->len * elem_size), src, (size_t)elem_size);
    vec->len++;
}

int64_t vanta_vec_pop(VantaVec *vec, int64_t elem_size) {
    if (vec == NULL || vec->len <= 0) {
        const char msg[] = "Cannot pop from an empty Vec";
        vanta_panic(msg, sizeof(msg) - 1);
    }
    if (elem_size <= 0) elem_size = 8;
    vec->len--;
    int64_t val = 0;
    memcpy(&val, (char *)vec->data + (vec->len * elem_size), (size_t)elem_size);
    return val;
}

void *vanta_vec_index(VantaVec *vec, int64_t index, int64_t elem_size) {
    if (vec == NULL || index < 0 || index >= vec->len) {
        int64_t len = vec ? vec->len : 0;
        vanta_bounds_check_fail(index, len);
    }
    if (elem_size <= 0) elem_size = 8;
    return (char *)vec->data + (index * elem_size);
}

void vanta_vec_free(VantaVec *vec) {
    if (vec != NULL) {
        if (vec->data != NULL) {
            vanta_free(vec->data);
            vec->data = NULL;
        }
        vec->len = 0;
        vec->cap = 0;
    }
}

int64_t vanta_vec_len(const VantaVec *vec) {
    return vec ? vec->len : 0;
}

int64_t vanta_vec_capacity(const VantaVec *vec) {
    return vec ? vec->cap : 0;
}

typedef struct {
    char *data;
    int64_t len;
    int64_t cap;
} VantaString;

void vanta_str_init(VantaString *s) {
    if (s != NULL) {
        s->data = NULL;
        s->len = 0;
        s->cap = 0;
    }
}

void vanta_str_free(VantaString *s) {
    if (s != NULL) {
        if (s->data != NULL) {
            vanta_free(s->data);
            s->data = NULL;
        }
        s->len = 0;
        s->cap = 0;
    }
}

void vanta_str_from_str(VantaString *dest, const char *ptr, int64_t len) {
    if (dest == NULL) return;
    if (ptr == NULL || len <= 0) {
        dest->data = NULL;
        dest->len = 0;
        dest->cap = 0;
        return;
    }
    if (len > 0x7fffffffffffffffLL) {
        vanta_panic("string allocation size overflow", 31);
    }
    dest->data = (char *)vanta_alloc((uint64_t)len);
    memcpy(dest->data, ptr, (size_t)len);
    dest->len = len;
    dest->cap = len;
}

void vanta_str_push_str(VantaString *s, const char *ptr, int64_t len) {
    if (s == NULL || ptr == NULL || len <= 0) return;

    int is_interior = (s->data != NULL && ptr >= s->data && ptr < (s->data + s->len));
    int64_t offset = is_interior ? (int64_t)(ptr - s->data) : 0;

    int64_t needed_len;
    if (__builtin_add_overflow(s->len, len, &needed_len) || needed_len < 0) {
        vanta_panic("string length overflow", 22);
    }

    if (needed_len > s->cap) {
        int64_t new_cap = (s->cap == 0) ? 8 : (s->cap * 2);
        if (new_cap < needed_len) {
            new_cap = needed_len;
        }
        if (new_cap < 0 || new_cap > 0x7fffffffffffffffLL) {
            vanta_panic("string capacity overflow", 24);
        }
        s->data = (char *)vanta_realloc(s->data, (uint64_t)new_cap);
        s->cap = new_cap;
        if (is_interior) {
            ptr = s->data + offset;
        }
    }

    memcpy(s->data + s->len, ptr, (size_t)len);
    s->len = needed_len;
}

int64_t vanta_str_len(const VantaString *s) {
    return s ? s->len : 0;
}

int64_t vanta_str_capacity(const VantaString *s) {
    return s ? s->cap : 0;
}

int64_t vanta_str_is_empty(const VantaString *s) {
    return (s == NULL || s->len == 0) ? 1 : 0;
}

void vanta_str_clear(VantaString *s) {
    if (s != NULL) {
        s->len = 0;
    }
}

void vanta_str_clone(VantaString *dest, const VantaString *src) {
    if (dest == NULL) return;
    if (src == NULL || src->data == NULL || src->len <= 0) {
        dest->data = NULL;
        dest->len = 0;
        dest->cap = 0;
        return;
    }
    dest->data = (char *)vanta_alloc((uint64_t)src->len);
    memcpy(dest->data, src->data, (size_t)src->len);
    dest->len = src->len;
    dest->cap = src->len;
}

int64_t vanta_str_equals(const char *p1, int64_t len1, const char *p2, int64_t len2) {
    if (len1 != len2) return 0;
    if (len1 == 0) return 1;
    if (p1 == NULL || p2 == NULL) return (p1 == p2) ? 1 : 0;
    return (memcmp(p1, p2, (size_t)len1) == 0) ? 1 : 0;
}

struct VantaOptionSliceRepr {
    int32_t tag;
    int32_t _pad;
    const char *ptr;
    int64_t len;
};

void vanta_str_slice_bytes(void *out_enum, const char *src_ptr, int64_t src_len, int64_t start, int64_t end) {
    struct VantaOptionSliceRepr *opt = (struct VantaOptionSliceRepr *)out_enum;
    if (opt == NULL) return;
    if (start < 0 || end < start || end > src_len) {
        opt->tag = 0;
        opt->_pad = 0;
        opt->ptr = NULL;
        opt->len = 0;
        return;
    }
    if (start > 0 && start < src_len && src_ptr != NULL) {
        unsigned char b = (unsigned char)src_ptr[start];
        if ((b & 0xC0) == 0x80) {
            opt->tag = 0;
            opt->_pad = 0;
            opt->ptr = NULL;
            opt->len = 0;
            return;
        }
    }
    if (end > 0 && end < src_len && src_ptr != NULL) {
        unsigned char b = (unsigned char)src_ptr[end];
        if ((b & 0xC0) == 0x80) {
            opt->tag = 0;
            opt->_pad = 0;
            opt->ptr = NULL;
            opt->len = 0;
            return;
        }
    }
    opt->tag = 1;
    opt->_pad = 0;
    opt->ptr = (src_ptr != NULL) ? (src_ptr + start) : NULL;
    opt->len = end - start;
}

void vanta_vec_free_strings(VantaVec *vec) {
    if (vec != NULL && vec->data != NULL) {
        VantaString *strs = (VantaString *)vec->data;
        for (int64_t i = 0; i < vec->len; i++) {
            vanta_str_free(&strs[i]);
        }
        vanta_free(vec->data);
        vec->data = NULL;
        vec->len = 0;
        vec->cap = 0;
    }
}

static int vanta_is_valid_utf8(const char *buf, size_t len) {
    size_t i = 0;
    while (i < len) {
        unsigned char c = (unsigned char)buf[i];
        if (c <= 0x7F) {
            i += 1;
        } else if ((c & 0xE0) == 0xC0) {
            if (i + 1 >= len) return 0;
            if (c < 0xC2) return 0; // reject overlong 2-byte sequence
            unsigned char c1 = (unsigned char)buf[i + 1];
            if ((c1 & 0xC0) != 0x80) return 0;
            i += 2;
        } else if ((c & 0xF0) == 0xE0) {
            if (i + 2 >= len) return 0;
            unsigned char c1 = (unsigned char)buf[i + 1];
            unsigned char c2 = (unsigned char)buf[i + 2];
            if ((c1 & 0xC0) != 0x80 || (c2 & 0xC0) != 0x80) return 0;
            if (c == 0xE0 && c1 < 0xA0) return 0; // reject overlong 3-byte
            if (c == 0xED && c1 >= 0xA0) return 0; // reject surrogate halves (U+D800..U+DFFF)
            i += 3;
        } else if ((c & 0xF8) == 0xF0) {
            if (i + 3 >= len) return 0;
            unsigned char c1 = (unsigned char)buf[i + 1];
            unsigned char c2 = (unsigned char)buf[i + 2];
            unsigned char c3 = (unsigned char)buf[i + 3];
            if ((c1 & 0xC0) != 0x80 || (c2 & 0xC0) != 0x80 || (c3 & 0xC0) != 0x80) return 0;
            if (c == 0xF0 && c1 < 0x90) return 0; // reject overlong 4-byte
            if (c == 0xF4 && c1 > 0x8F) return 0; // reject codepoints > U+10FFFF
            if (c > 0xF4) return 0;
            i += 4;
        } else {
            return 0;
        }
    }
    return 1;
}

void vanta_io_write_stdout(const char *ptr, uint64_t len) {
    if (ptr != NULL && len > 0) {
        safe_write_all(1, ptr, (size_t)len);
    }
}

void vanta_io_write_stderr(const char *ptr, uint64_t len) {
    if (ptr != NULL && len > 0) {
        safe_write_all(2, ptr, (size_t)len);
    }
}

int64_t vanta_io_read_stdin(VantaString *dest) {
    if (dest == NULL) return -1;
    dest->data = NULL;
    dest->len = 0;
    dest->cap = 0;

    size_t cap = 512;
    char *buf = (char *)malloc(cap);
    if (buf == NULL) return -1002;

    size_t total = 0;
    while (1) {
        if (total >= cap) {
            size_t new_cap = cap * 2;
            char *new_buf = (char *)realloc(buf, new_cap);
            if (new_buf == NULL) {
                free(buf);
                return -1002;
            }
            buf = new_buf;
            cap = new_cap;
        }
        ssize_t n = read(0, buf + total, cap - total);
        if (n > 0) {
            total += (size_t)n;
        } else if (n == 0) {
            break;
        } else {
            if (errno == EINTR) continue;
            int r_err = errno;
            free(buf);
            return r_err > 0 ? -r_err : -1;
        }
    }

    if (!vanta_is_valid_utf8(buf, total)) {
        free(buf);
        return -1001;
    }

    if (total == 0) {
        free(buf);
        dest->data = NULL;
        dest->len = 0;
        dest->cap = 0;
        return 0;
    }

    dest->data = (char *)vanta_alloc((uint64_t)total);
    memcpy(dest->data, buf, total);
    dest->len = (int64_t)total;
    dest->cap = (int64_t)total;
    free(buf);
    return 0;
}

int64_t vanta_fs_read_to_string(const char *path, uint64_t path_len, VantaString *dest) {
    if (dest == NULL) return -1;
    dest->data = NULL;
    dest->len = 0;
    dest->cap = 0;
    if (path == NULL) return -1;

    char stack_path[1024];
    char *p_path = stack_path;
    if (path_len >= sizeof(stack_path)) {
        p_path = (char *)malloc((size_t)path_len + 1);
        if (p_path == NULL) return -1002;
    }
    memcpy(p_path, path, (size_t)path_len);
    p_path[path_len] = '\0';

    int fd = open(p_path, O_RDONLY);
    int open_err = errno;
    if (p_path != stack_path) {
        free(p_path);
    }
    if (fd < 0) {
        return open_err > 0 ? -open_err : -1;
    }

    struct stat st;
    size_t init_cap = 512;
    if (fstat(fd, &st) == 0 && S_ISREG(st.st_mode) && st.st_size > 0) {
        init_cap = (size_t)st.st_size;
    }

    size_t cap = init_cap;
    char *buf = (char *)malloc(cap);
    if (buf == NULL) {
        close(fd);
        return -1002;
    }

    size_t total = 0;
    while (1) {
        if (total >= cap) {
            size_t new_cap = cap * 2;
            char *new_buf = (char *)realloc(buf, new_cap);
            if (new_buf == NULL) {
                free(buf);
                close(fd);
                return -1002;
            }
            buf = new_buf;
            cap = new_cap;
        }
        ssize_t n = read(fd, buf + total, cap - total);
        if (n > 0) {
            total += (size_t)n;
        } else if (n == 0) {
            break;
        } else {
            if (errno == EINTR) continue;
            int r_err = errno;
            free(buf);
            close(fd);
            return r_err > 0 ? -r_err : -1;
        }
    }
    close(fd);

    if (!vanta_is_valid_utf8(buf, total)) {
        free(buf);
        return -1001;
    }

    if (total == 0) {
        free(buf);
        dest->data = NULL;
        dest->len = 0;
        dest->cap = 0;
        return 0;
    }

    dest->data = (char *)vanta_alloc((uint64_t)total);
    memcpy(dest->data, buf, total);
    dest->len = (int64_t)total;
    dest->cap = (int64_t)total;
    free(buf);
    return 0;
}

int64_t vanta_fs_write(const char *path, uint64_t path_len, const char *data, uint64_t data_len) {
    if (path == NULL) return -1;
    char stack_path[1024];
    char *p_path = stack_path;
    if (path_len >= sizeof(stack_path)) {
        p_path = (char *)malloc((size_t)path_len + 1);
        if (p_path == NULL) return -1002;
    }
    memcpy(p_path, path, (size_t)path_len);
    p_path[path_len] = '\0';

    int fd = open(p_path, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    int open_err = errno;
    if (p_path != stack_path) {
        free(p_path);
    }
    if (fd < 0) {
        return open_err > 0 ? -open_err : -1;
    }

    if (data != NULL && data_len > 0) {
        size_t written = 0;
        while (written < (size_t)data_len) {
            ssize_t n = write(fd, data + written, (size_t)data_len - written);
            if (n > 0) {
                written += (size_t)n;
            } else if (n < 0) {
                if (errno == EINTR) continue;
                int w_err = errno;
                close(fd);
                return w_err > 0 ? -w_err : -1;
            } else {
                break;
            }
        }
    }
    close(fd);
    return (int64_t)data_len;
}

int64_t vanta_env_args_count(void) {
    return (int64_t)g_argc;
}

int64_t vanta_env_arg_get(int64_t idx, VantaString *dest) {
    if (dest == NULL) return -1;
    dest->data = NULL;
    dest->len = 0;
    dest->cap = 0;

    if (idx < 0 || idx >= g_argc || g_argv == NULL || g_argv[idx] == NULL) {
        return -1;
    }
    const char *arg = g_argv[idx];
    size_t len = strlen(arg);
    if (!vanta_is_valid_utf8(arg, len)) {
        return -1001;
    }
    vanta_str_from_str(dest, arg, (int64_t)len);
    return 0;
}


/* ============================================================================
 * Vanta Networking & HTTP Server Primitives
 * ============================================================================ */
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int64_t vanta_net_listen(int64_t port) {
    int server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) return -1;

    int opt = 1;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port = htons((uint16_t)port);

    if (bind(server_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        close(server_fd);
        return -2;
    }

    if (listen(server_fd, 32) < 0) {
        close(server_fd);
        return -3;
    }

    return (int64_t)server_fd;
}

int64_t vanta_net_accept(int64_t server_fd) {
    if (server_fd < 0) return -1;
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept((int)server_fd, (struct sockaddr *)&client_addr, &client_len);
    return (int64_t)client_fd;
}

int64_t vanta_net_read(int64_t client_fd, VantaString *dest) {
    if (dest == NULL) return -1;
    dest->data = NULL;
    dest->len = 0;
    dest->cap = 0;

    char buf[4096];
    ssize_t n = recv((int)client_fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) return -1;
    buf[n] = '\0';
    vanta_str_from_str(dest, buf, (int64_t)n);
    return (int64_t)n;
}

int64_t vanta_net_write(int64_t client_fd, const char *data, uint64_t len) {
    if (data == NULL || len == 0) return 0;
    size_t total = 0;
    while (total < (size_t)len) {
        ssize_t n = send((int)client_fd, data + total, (size_t)len - total, 0);
        if (n <= 0) {
            if (errno == EINTR) continue;
            break;
        }
        total += (size_t)n;
    }
    return (int64_t)total;
}

int64_t vanta_net_close(int64_t fd) {
    if (fd >= 0) {
        close((int)fd);
    }
    return 0;
}

#if defined(__x86_64__) || defined(_M_X64)
#include <cpuid.h>

int64_t vanta_cpu_has_avx(void) {
    unsigned int eax, ebx, ecx, edx;
    if (__get_cpuid(1, &eax, &ebx, &ecx, &edx)) {
        if ((ecx & (1 << 28)) && (ecx & (1 << 27))) {
            return 1;
        }
    }
    return 0;
}

int64_t vanta_cpu_has_avx2(void) {
    unsigned int eax, ebx, ecx, edx;
    if (__get_cpuid_count(7, 0, &eax, &ebx, &ecx, &edx)) {
        if (ebx & (1 << 5)) {
            return 1;
        }
    }
    return 0;
}

int64_t vanta_cpu_has_fma(void) {
    unsigned int eax, ebx, ecx, edx;
    if (__get_cpuid(1, &eax, &ebx, &ecx, &edx)) {
        if (ecx & (1 << 12)) {
            return 1;
        }
    }
    return 0;
}

int64_t vanta_cpu_has_avx512f(void) {
    unsigned int eax, ebx, ecx, edx;
    if (__get_cpuid_count(7, 0, &eax, &ebx, &ecx, &edx)) {
        if (ebx & (1 << 16)) {
            return 1;
        }
    }
    return 0;
}

#include <immintrin.h>

__attribute__((target("avx2")))
__m256i vanta_c_vector_add(__m256i a, __m256i b) {
    return _mm256_add_epi64(a, b);
}
#else
int64_t vanta_cpu_has_avx(void) { return 0; }
int64_t vanta_cpu_has_avx2(void) { return 0; }
int64_t vanta_cpu_has_fma(void) { return 0; }
int64_t vanta_cpu_has_avx512f(void) { return 0; }
#endif

