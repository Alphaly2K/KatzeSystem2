#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <xmmintrin.h>

static uint32_t estimate(uint32_t input, int square_root) {
    float value;
    memcpy(&value, &input, sizeof(value));
    __m128 vector = _mm_set_ss(value);
    vector = square_root ? _mm_rsqrt_ss(vector) : _mm_rcp_ss(vector);
    value = _mm_cvtss_f32(vector);
    uint32_t result;
    memcpy(&result, &value, sizeof(result));
    return result;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fputs("usage: generate-sse-estimates OUTPUT\n", stderr);
        return 1;
    }
    uint8_t table[24576];
    size_t position = 0;
    for (int part = 0; part < 3; ++part) {
        uint32_t exponent = part == 2 ? 128 : 127;
        for (uint32_t bucket = 0; bucket < 4096; ++bucket) {
            uint32_t input = (exponent << 23) | (bucket << 11);
            uint32_t result = estimate(input, part != 0);
            // 仅接受当前压缩布局可表示、且桶内不变的估计值。
            if ((result & 0xff8007ff) != 0x3f000000 ||
                result != estimate(input + 1, part != 0) ||
                result != estimate(input + 1023, part != 0) ||
                result != estimate(input + 2047, part != 0)) {
                fputs("SSE estimates do not fit the baseline table layout\n", stderr);
                return 1;
            }
            uint16_t fraction = (uint16_t)((result >> 11) & 4095);
            table[position++] = (uint8_t)fraction;
            table[position++] = (uint8_t)(fraction >> 8);
        }
    }
    FILE *output = fopen(argv[1], "wb");
    if (output == NULL) {
        perror("open output");
        return 1;
    }
    int written = fwrite(table, 1, sizeof(table), output) == sizeof(table);
    int closed = fclose(output) == 0;
    if (!written || !closed) {
        fputs("failed to write SSE estimate table\n", stderr);
        return 1;
    }
    return 0;
}
