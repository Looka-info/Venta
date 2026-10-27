#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

typedef struct {
    char *data;
    int64_t len;
    int64_t cap;
} CString;

void str_init(CString *s, const char *init_str) {
    int64_t len = strlen(init_str);
    s->data = (char *)malloc(len + 1);
    memcpy(s->data, init_str, len + 1);
    s->len = len;
    s->cap = len;
}

void str_push(CString *s, const char *chunk) {
    int64_t clen = strlen(chunk);
    if (s->len + clen > s->cap) {
        int64_t new_cap = (s->cap == 0) ? 8 : s->cap * 2;
        if (new_cap < s->len + clen) new_cap = s->len + clen;
        s->data = (char *)realloc(s->data, new_cap + 1);
        s->cap = new_cap;
    }
    memcpy(s->data + s->len, chunk, clen);
    s->len += clen;
    s->data[s->len] = '\0';
}

void str_free(CString *s) {
    free(s->data);
    s->data = NULL;
    s->len = 0;
    s->cap = 0;
}

__attribute__((noinline))
int64_t make_string(int64_t n) {
    CString s;
    str_init(&s, "base_");
    for (int64_t i = 0; i < n; i++) {
        str_push(&s, "chunk_");
    }
    int64_t len = s.len;
    str_free(&s);
    return len;
}

int main() {
    int64_t total_len = 0;
    for (int64_t iter = 0; iter < 2000; iter++) {
        total_len += make_string(10);
    }
    printf("%ld\n", total_len);
    return 0;
}
