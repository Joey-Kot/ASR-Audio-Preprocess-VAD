#ifndef SMARTAUDIO_NATIVE_H
#define SMARTAUDIO_NATIVE_H
#include <stdint.h>
typedef struct SaReader SaReader;
typedef struct SaWriter SaWriter;
typedef int (*SaCancel)(void *);
typedef int (*SaSamples)(void *, const int16_t *, int);
typedef struct SaInfo {
    int rate, channels, stream;
    int64_t frames;
    int64_t bitrate;
    int bits_per_sample;
    char codec[64], sample_format[32];
} SaInfo;
SaReader *sa_open(const char *, const char *, int, int, int, SaCancel, void *, SaInfo *, char *);
int sa_next(SaReader *, int64_t *, int *, char *);
int64_t sa_start_skip(SaReader *);
int sa_analyze(SaReader *, SaSamples, void *, int64_t *, char *);
void sa_reader_free(SaReader *);
SaWriter *sa_writer_open(SaReader *, const char *, const char *, const char *, int, int, int64_t, const char *, int, char *);
int sa_write(SaWriter *, SaReader *, int, int, char *);
int sa_finish(SaWriter *, SaInfo *, char *);
int sa_encoder_delay(SaWriter *);
void sa_writer_free(SaWriter *);
int sa_supported(const char *, const char *);
const char *sa_version(void);
void sa_logging_init(void);
char *sa_logging_scope(char *);
#endif
