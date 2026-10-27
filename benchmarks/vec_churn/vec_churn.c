#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

typedef struct {
    int64_t *data;
    int64_t len;
    int64_t cap;
} CVector;

void vec_init(CVector *v) {
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}

void vec_push(CVector *v, int64_t val) {
    if (v->len >= v->cap) {
        int64_t new_cap = v->cap == 0 ? 4 : v->cap * 2;
        v->data = (int64_t *)realloc(v->data, new_cap * sizeof(int64_t));
        v->cap = new_cap;
    }
    v->data[v->len++] = val;
}

void vec_free(CVector *v) {
    free(v->data);
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}

__attribute__((noinline))
int64_t churn(int64_t count) {
    CVector v;
    vec_init(&v);
    for (int64_t i = 0; i < count; i++) {
        vec_push(&v, i);
    }
    int64_t sum = 0;
    for (int64_t j = 0; j < v.len; j++) {
        sum += v.data[j];
    }
    vec_free(&v);
    return sum;
}

int main() {
    int64_t total = 0;
    for (int64_t iter = 0; iter < 200; iter++) {
        total += churn(10000);
    }
    printf("%ld\n", total);
    return 0;
}
