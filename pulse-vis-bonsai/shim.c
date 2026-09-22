/*
 * pulse_viz_shim.c - C shim for PulseAudio TUI visualizer (pulse-viz).
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <pthread.h>

#include <pulse/pulse.h>

/* ---------- opaque handles ---------- */
typedef void *pvz_context;
typedef void *pvz_stream;
typedef void *pvz_mainloop;
typedef void *pvz_operation;

/* ---------- exported data structs ---------- */
typedef struct {
    char        name[512];
    char        alias[256];
    uint32_t    source;
    int         channels;
    int         rate;
    int         sample_format;
    uint32_t    client;
    uint32_t    index;
    int         mute;
} pvz_source_info_t;

/* ---------- internal structs ---------- */
typedef struct {
    pa_context  *ctx;
    pa_mainloop *ml;
} ctx_wrap;

typedef struct {
    pa_stream  *stream;
    int         rate;
    int         channels;
    int         sample_format;
    char        source_name[512];
} stream_wrap;

typedef struct {
    pa_operation *op;
    pvz_source_info_t list[256];
    int            count;
    int            done;
    ctx_wrap      *ctx_wrap;
} src_op;

/* ---------- mainloop ---------- */
pvz_mainloop pulse_viz_mainloop_new(void) {
    return (pvz_mainloop)pa_mainloop_new();
}

int pulse_viz_mainloop_iterate(pvz_mainloop ml, int block, int *retval) {
    if (!ml || !retval) return -1;
    return pa_mainloop_iterate((pa_mainloop *)ml, block, retval);
}

void pulse_viz_mainloop_free(pvz_mainloop ml) {
    if (!ml) return;
    pa_mainloop_free((pa_mainloop *)ml);
}

/* ---------- context ---------- */
pvz_context pulse_viz_context_new(pvz_mainloop ml) {
    if (!ml) return NULL;
    pa_mainloop *pm = (pa_mainloop *)ml;
    pa_context *ctx = pa_context_new((pa_mainloop_api *)pm, NULL);
    if (!ctx) return NULL;
    ctx_wrap *w = (ctx_wrap *)calloc(1, sizeof(*w));
    if (!w) { pa_context_unref(ctx); return NULL; }
    w->ctx = ctx;
    w->ml = pm;
    return (pvz_context)w;
}

void pulse_viz_context_free(pvz_context c) {
    if (!c) return;
    ctx_wrap *w = (ctx_wrap *)c;
    if (w->ctx) pa_context_unref(w->ctx);
    free(w);
}

int pulse_viz_context_connect(pvz_context c) {
    if (!c) return -1;
    ctx_wrap *w = (ctx_wrap *)c;
    /*
     * libpulsecommon-17.0.so bug: pa_context_connect crashes (NULL deref
     * in pa_socket_client_new_sockaddr) when passed a non-NULL, non-empty
     * server string. The workaround is to pass an empty string (which is
     * safe) and set PULSE_SERVER so PA resolves the actual socket path.
     */
    const char *default_server = "/run/user/1000/pulse/native";
    const char *env = getenv("PULSE_SERVER");
    if (!env || env[0] == '\0') {
        if (getenv("PULSE_SERVER") == NULL) {
            setenv("PULSE_SERVER", default_server, 1);
        } else {
            setenv("PULSE_SERVER", "", 0);
        }
    }
    return pa_context_connect(w->ctx, "", 0, NULL);
}

int pulse_viz_context_errno(pvz_context c) {
    if (!c) return -1;
    return pa_context_errno(((ctx_wrap *)c)->ctx);
}

int pulse_viz_context_state(pvz_context c) {
    if (!c) return -1;
    return (int)pa_context_get_state(((ctx_wrap *)c)->ctx);
}

/* ---------- stream ---------- */
pvz_stream pulse_viz_stream_new(pvz_context c, const char *source_name,
                                 int rate, int channels, int sample_format) {
    if (!c) return NULL;
    ctx_wrap *w = (ctx_wrap *)c;
    const char *src = (source_name && source_name[0]) ? source_name : NULL;
    pa_sample_spec ss = { .rate = rate, .channels = channels, .format = sample_format };
    pa_stream *s = pa_stream_new(w->ctx, src, &ss, NULL);
    if (!s) return NULL;
    stream_wrap *sw = (stream_wrap *)calloc(1, sizeof(*sw));
    if (!sw) { pa_stream_unref(s); return NULL; }
    sw->stream = s;
    sw->rate = rate;
    sw->channels = channels;
    sw->sample_format = sample_format;
    memset(sw->source_name, 0, sizeof(sw->source_name));
    if (source_name) strncpy(sw->source_name, source_name, sizeof(sw->source_name)-1);
    return (pvz_stream)sw;
}

void pulse_viz_stream_free(pvz_stream s) {
    if (!s) return;
    stream_wrap *sw = (stream_wrap *)s;
    pa_stream_unref(sw->stream);
    free(sw);
}

int pulse_viz_stream_get_state(pvz_stream s) {
    if (!s) return -1;
    return (int)pa_stream_get_state(((stream_wrap *)s)->stream);
}

int pulse_viz_stream_get_rate(pvz_stream s) {
    if (!s) return -1;
    stream_wrap *sw = (stream_wrap *)s;
    const pa_format_info *fi = pa_stream_get_format_info(sw->stream);
    if (!fi) return -1;
    pa_sample_spec spec;
    pa_channel_map map;
    if (pa_format_info_to_sample_spec(fi, &spec, &map) != 0) return -1;
    return spec.rate;
}

int pulse_viz_stream_get_channels(pvz_stream s) {
    if (!s) return -1;
    const pa_channel_map *cm = pa_stream_get_channel_map(((stream_wrap *)s)->stream);
    return cm ? (int)cm->channels : -1;
}

int pulse_viz_stream_get_sample_format(pvz_stream s) {
    if (!s) return -1;
    stream_wrap *sw = (stream_wrap *)s;
    const pa_format_info *fi = pa_stream_get_format_info(sw->stream);
    if (!fi) return -1;
    pa_sample_spec spec;
    pa_channel_map map;
    if (pa_format_info_to_sample_spec(fi, &spec, &map) != 0) return -1;
    return (int)spec.format;
}

size_t pulse_viz_stream_readable_size(pvz_stream s) {
    if (!s) return 0;
    size_t n = pa_stream_readable_size(((stream_wrap *)s)->stream);
    return (n == SIZE_MAX) ? 0 : n;
}

size_t pulse_viz_stream_read(pvz_stream s, void *buf, size_t buf_size) {
    if (!s || !buf || buf_size == 0) return 0;
    stream_wrap *sw = (stream_wrap *)s;
    if (!sw->stream) return 0;
    size_t avail = pa_stream_readable_size(sw->stream);
    if (avail == SIZE_MAX || avail == 0) return 0;
    if (avail > buf_size) avail = buf_size;
    size_t n = 0;
    if (pa_stream_peek(sw->stream, buf, &n) != 0) return 0;
    if (n == 0) return 0;
    pa_stream_drop(sw->stream);
    return n;
}

static void pvz_read_cb(pa_stream *s, size_t nbytes, void *u) {
    (void)s; (void)nbytes; (void)u;
}
void pulse_viz_stream_set_read_callback(pvz_stream s) {
    if (!s) return;
    pa_stream_set_read_callback(((stream_wrap *)s)->stream, pvz_read_cb, NULL);
}

static void pvz_state_cb(pa_stream *s, void *u) {
    (void)s; (void)u;
}
void pulse_viz_stream_set_state_callback(pvz_stream s) {
    if (!s) return;
    pa_stream_set_state_callback(((stream_wrap *)s)->stream, pvz_state_cb, NULL);
}

/* ---------- source enumeration ---------- */

static void pvz_src_info_cb(pa_context *c, const pa_source_info *si, int eol, void *u) {
    (void)c;
    if (eol) return;
    src_op *op = (src_op *)u;
    if (op->count < 256) {
        pvz_source_info_t *dst = &op->list[op->count];
        if (si->name) strncpy(dst->name, si->name, sizeof(dst->name)-1);
        else strcpy(dst->name, "unknown");
        dst->name[sizeof(dst->name)-1] = 0;
        dst->alias[0] = 0;
        if (si->proplist) {
            const char *alias = pa_proplist_gets(si->proplist, "source-alias");
            if (alias) {
                strncpy(dst->alias, alias, sizeof(dst->alias)-1);
                dst->alias[sizeof(dst->alias)-1] = 0;
            }
        }
        dst->source = si->index;
        dst->channels = (int)si->channel_map.channels;
        dst->rate = si->sample_spec.rate;
        dst->sample_format = (int)si->sample_spec.format;
        dst->client = si->owner_module;
        dst->index = si->index;
        dst->mute = si->mute ? 1 : 0;
        op->count++;
    }
}

pvz_operation pulse_viz_start_source_enum(pvz_context c) {
    if (!c) return NULL;
    ctx_wrap *w = (ctx_wrap *)c;
    src_op *op = (src_op *)calloc(1, sizeof(*op));
    if (!op) return NULL;
    op->ctx_wrap = w;
    op->count = 0;
    op->done = 0;
    op->op = pa_context_get_source_info_list(w->ctx,
                                               (pa_source_info_cb_t)pvz_src_info_cb,
                                               (void *)op);
    if (!op->op) { free(op); return NULL; }
    return (pvz_operation)op;
}

int pulse_viz_get_src_op_state(pvz_operation op) {
    if (!op) return -1;
    src_op *so = (src_op *)op;
    if (!so->op) return 2;
    return (int)pa_operation_get_state(so->op);
}

const pvz_source_info_t *pulse_viz_get_src_op_list(pvz_operation op, int *count_out) {
    if (!op || !count_out) return NULL;
    src_op *so = (src_op *)op;
    *count_out = so->count;
    return &so->list[0];
}

void pulse_viz_free_src_op(pvz_operation op) {
    if (!op) return;
    src_op *so = (src_op *)op;
    if (so->op) pa_operation_unref(so->op);
    free(so);
}

int pulse_viz_op_get_state(pvz_operation op) {
    if (op) return 0;
    return -1;
}
void pulse_viz_op_unref(pvz_operation op) {
    if (op) { (void)op; }
}
void pulse_viz_op_cancel(pvz_operation op) {
    if (op) { (void)op; }
}
