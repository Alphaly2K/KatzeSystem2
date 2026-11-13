#include <stddef.h>
#define PLM_NO_STDIO
#define PL_MPEG_IMPLEMENTATION
#include "pl_mpeg.h"

/* Keep the source length intact when metadata inspection seeks past EOF. */
void katze_movie_buffer_end(plm_buffer_t *buffer) {
    buffer->has_ended = TRUE;
}
