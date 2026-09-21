/* Copyright (C) 2026 Joey Kot. GPL-3.0-or-later.
 * Sample selection follows STT-for-Windows's native sample-copy approach.
 * No filtergraph, subprocess, intermediate WAV, or full-input PCM buffer.
 */
#include "audio.h"
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdarg.h>
#include <string.h>
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/audio_fifo.h>
#include <libavutil/channel_layout.h>
#include <libavutil/opt.h>
#include <libavutil/intreadwrite.h>
#include <libavutil/samplefmt.h>
#include <libswresample/swresample.h>

/* The embedded libav logger is installed once. Scoped, thread-local capture
 * keeps independent jobs' diagnostics separate and never calls user code.
 * Calls outside SmartAudio scopes retain libav's default logging behavior.
 */
static _Thread_local char *log_sink;
static void capture_log(void *context,int level,const char *format,va_list args) {
    if(level>av_log_get_level())return;
    if(log_sink) {
        if(level<=AV_LOG_WARNING)vsnprintf(log_sink,1024,format,args);
    } else av_log_default_callback(context,level,format,args);
}
void sa_logging_init(void) { av_log_set_callback(capture_log); }
char *sa_logging_scope(char *sink) {char *previous=log_sink;log_sink=sink;return previous;}

struct SaReader {
    AVFormatContext *format;
    AVCodecContext *decoder;
    AVPacket *packet;
    AVFrame *frame;
    int stream, draining, primed, rate, sample_format;
    AVChannelLayout layout;
    int64_t position, start_skip;
    SaCancel cancel;
    void *context;
};
struct SaWriter {
    AVFormatContext *format;
    AVCodecContext *encoder;
    AVStream *stream;
    SwrContext *swr;
    AVAudioFifo *fifo;
    int source_rate, finished, initial_vorbis_packets;
    int64_t ogg_page_duration;
    int64_t pts, valid_frames, source_frames, final_frames;
    SaReader *reader;
};
static int failure(char *error, const char *operation, int code) {
    char detail[AV_ERROR_MAX_STRING_SIZE];
    av_strerror(code, detail, sizeof(detail));
    snprintf(error, 1024, "%s: %s", operation, detail);
    if(log_sink && *log_sink) {
        size_t n=strlen(error);
        snprintf(error+n,1024-n," (%s)",log_sink);
    }
    return code;
}
static int interrupt(void *context) {
    SaReader *r = context;
    return r->cancel && r->cancel(r->context);
}
const char *sa_version(void) { return av_version_info(); }
int sa_supported(const char *muxer, const char *codec) {
    const AVOutputFormat *f = av_guess_format(muxer, NULL, NULL);
    const AVCodec *c = avcodec_find_encoder_by_name(codec);
    if (!f || !c) return 0;
    /* Some raw muxers do not implement query_codec; the Rust registry additionally
     * restricts pairs, then opening/writing the actual muxer is authoritative. */
    return avformat_query_codec(f,c->id,FF_COMPLIANCE_NORMAL) != 0;
}
static int receive(SaReader *r, char *error) {
    for (;;) {
        if (interrupt(r)) return failure(error,"cancelled",AVERROR_EXIT);
        int ret = avcodec_receive_frame(r->decoder, r->frame);
        if (ret >= 0) return 1;
        if (ret == AVERROR_EOF) return 0;
        if (ret != AVERROR(EAGAIN)) return failure(error,"decode frame",ret);
        if (r->draining) return failure(error,"decoder did not finish draining",AVERROR_INVALIDDATA);
        do {
            av_packet_unref(r->packet);
            ret = av_read_frame(r->format,r->packet);
        } while (ret >= 0 && r->packet->stream_index != r->stream && !interrupt(r));
        if (interrupt(r)) return failure(error,"cancelled",AVERROR_EXIT);
        if (ret == AVERROR_EOF) {
            r->draining = 1;
            ret = avcodec_send_packet(r->decoder,NULL);
        } else if (ret >= 0) {
            if(!r->position) {
                size_t side_size=0;
                const uint8_t *side=av_packet_get_side_data(r->packet,AV_PKT_DATA_SKIP_SAMPLES,&side_size);
                if(side && side_size>=10)r->start_skip+=AV_RL32(side);
            }
            ret = avcodec_send_packet(r->decoder,r->packet);
        }
        av_packet_unref(r->packet);
        if (ret < 0) return failure(error,"read/send packet",ret);
    }
}
int64_t sa_start_skip(SaReader *r) { return r->start_skip; }
int sa_encoder_delay(SaWriter *w) {
    /* Speex also has a decoder lookahead of one 5 ms subframe at each rate. */
    return w->encoder->initial_padding +
        (w->encoder->codec_id==AV_CODEC_ID_SPEEX ? w->encoder->sample_rate/200 : 0);
}
void sa_reader_free(SaReader *r) {
    if (!r) return;
    av_channel_layout_uninit(&r->layout);
    av_frame_free(&r->frame); av_packet_free(&r->packet);
    avcodec_free_context(&r->decoder); avformat_close_input(&r->format); av_free(r);
}
SaReader *sa_open(const char *path, const char *raw, int raw_rate, int raw_channels,
    int stream, SaCancel cancel, void *context, SaInfo *info, char *error) {
    SaReader *r = av_mallocz(sizeof(*r));
    AVDictionary *options = NULL;
    int ret = AVERROR(ENOMEM);
    if (!r) { failure(error,"allocate reader",ret); return NULL; }
    r->cancel=cancel; r->context=context;
    r->format=avformat_alloc_context();
    if (!r->format) goto fail;
    r->format->interrupt_callback=(AVIOInterruptCB){interrupt,r};
    const AVInputFormat *input = NULL;
    if (raw && *raw) {
        input=av_find_input_format(raw);
        if (!input) { ret=AVERROR_DEMUXER_NOT_FOUND; goto fail; }
        av_dict_set_int(&options,"sample_rate",raw_rate,0);
        AVChannelLayout layout; char description[128];
        av_channel_layout_default(&layout,raw_channels);
        av_channel_layout_describe(&layout,description,sizeof(description));
        av_channel_layout_uninit(&layout);
        av_dict_set(&options,"ch_layout",description,0);
    }
    ret=avformat_open_input(&r->format,path,input,&options);
    av_dict_free(&options);
    if(ret<0) goto fail;
    ret=avformat_find_stream_info(r->format,NULL); if(ret<0) goto fail;
    if (stream < 0) stream=av_find_best_stream(r->format,AVMEDIA_TYPE_AUDIO,-1,-1,NULL,0);
    if(stream<0 || stream >= (int)r->format->nb_streams || r->format->streams[stream]->codecpar->codec_type!=AVMEDIA_TYPE_AUDIO) {
        ret=AVERROR_STREAM_NOT_FOUND; goto fail;
    }
    r->stream=stream;
    const AVCodec *codec=avcodec_find_decoder(r->format->streams[stream]->codecpar->codec_id);
    if(!codec) {ret=AVERROR_DECODER_NOT_FOUND;goto fail;}
    r->decoder=avcodec_alloc_context3(codec); if(!r->decoder){ret=AVERROR(ENOMEM);goto fail;}
    ret=avcodec_parameters_to_context(r->decoder,r->format->streams[stream]->codecpar); if(ret<0)goto fail;
    r->decoder->thread_count=1;
    r->decoder->log_level_offset=128;
    r->decoder->pkt_timebase=r->format->streams[stream]->time_base;
    r->decoder->err_recognition=AV_EF_CAREFUL | AV_EF_EXPLODE;
    ret=avcodec_open2(r->decoder,codec,NULL); if(ret<0)goto fail;
    r->packet=av_packet_alloc(); r->frame=av_frame_alloc();
    if(!r->packet || !r->frame){ret=AVERROR(ENOMEM);goto fail;}
    ret=receive(r,error); if(ret<0)goto fail;
    r->primed=ret;
    r->rate=ret ? r->frame->sample_rate : r->decoder->sample_rate;
    r->sample_format=ret ? r->frame->format : r->decoder->sample_fmt;
    ret=av_channel_layout_copy(&r->layout,r->primed ? &r->frame->ch_layout : &r->decoder->ch_layout);
    if(ret<0 || r->rate<=0 || r->layout.nb_channels<=0){ret=AVERROR_INVALIDDATA;goto fail;}
    memset(info,0,sizeof(*info)); info->rate=r->rate; info->channels=r->layout.nb_channels; info->stream=stream;
    info->bitrate=r->decoder->bit_rate;
    info->bits_per_sample=r->decoder->bits_per_raw_sample ? r->decoder->bits_per_raw_sample : av_get_bits_per_sample(r->decoder->codec_id);
    snprintf(info->codec,sizeof(info->codec),"%s",codec->name);
    const char *fmt=av_get_sample_fmt_name(r->sample_format);
    snprintf(info->sample_format,sizeof(info->sample_format),"%s",fmt?fmt:"unknown");
    return r;
fail:
    av_dict_free(&options); failure(error,"open input",ret); sa_reader_free(r); return NULL;
}
int sa_next(SaReader *r,int64_t *position,int *count,char *error) {
    int ret;
    if(interrupt(r))return failure(error,"cancelled",AVERROR_EXIT);
    if(r->primed){r->primed=0;ret=1;} else {av_frame_unref(r->frame);ret=receive(r,error);}
    if(ret<=0)return ret;
    if(r->frame->sample_rate!=r->rate || r->frame->format!=r->sample_format ||
       av_channel_layout_compare(&r->frame->ch_layout,&r->layout))
        return failure(error,"input format changed",AVERROR_INVALIDDATA);
    if(r->frame->nb_samples > INT64_MAX-r->position)return failure(error,"source position overflow",AVERROR(EOVERFLOW));
    *position=r->position; *count=r->frame->nb_samples; r->position += *count;
    return 1;
}
int sa_analyze(SaReader *r,SaSamples samples,void *context,int64_t *frames,char *error) {
    SwrContext *swr=NULL; AVChannelLayout mono=AV_CHANNEL_LAYOUT_MONO;
    int ret=swr_alloc_set_opts2(&swr,&mono,AV_SAMPLE_FMT_S16,16000,&r->layout,r->sample_format,r->rate,0,NULL);
    if(ret<0)goto done;
    ret=swr_init(swr); if(ret<0)goto done;
    int64_t position; int n,capacity; int16_t *buffer=NULL;
    while((ret=sa_next(r,&position,&n,error))>0) {
        int64_t size=av_rescale_rnd(swr_get_delay(swr,r->rate)+n,16000,r->rate,AV_ROUND_UP);
        if(size>INT_MAX){ret=AVERROR(EOVERFLOW);break;}
        capacity=(int)size;
        buffer=av_malloc_array(capacity,sizeof(*buffer)); if(!buffer){ret=AVERROR(ENOMEM);break;}
        uint8_t *planes[]={(uint8_t *)buffer};
        ret=swr_convert(swr,planes,capacity,(const uint8_t **)r->frame->extended_data,n);
        if(ret>=0 && samples(context,buffer,ret))ret=AVERROR_EXTERNAL;
        av_freep(&buffer); if(ret<0)break;
    }
    if(ret==0) {
        int16_t tail[4096]; uint8_t *planes[]={(uint8_t *)tail};
        while((ret=swr_convert(swr,planes,4096,NULL,0))>0) {
            if(interrupt(r)){ret=AVERROR_EXIT;break;}
            if(samples(context,tail,ret)){ret=AVERROR_EXTERNAL;break;}
        }
    }
    *frames=r->position;
done:
    swr_free(&swr);
    return ret<0?failure(error,"analysis",ret):0;
}
static AVFrame *frame_alloc(enum AVSampleFormat format,const AVChannelLayout *layout,int rate,int count) {
    AVFrame *f=av_frame_alloc(); if(!f)return NULL;
    f->format=format; f->sample_rate=rate; f->nb_samples=count;
    if(av_channel_layout_copy(&f->ch_layout,layout)<0 || av_frame_get_buffer(f,0)<0)av_frame_free(&f);
    return f;
}
static int encode(SaWriter *w,AVFrame *f,char *error) {
    int ret=avcodec_send_frame(w->encoder,f); if(ret<0)return failure(error,"encode frame",ret);
    AVPacket *p=av_packet_alloc(); if(!p)return failure(error,"allocate packet",AVERROR(ENOMEM));
    while((ret=avcodec_receive_packet(w->encoder,p))>=0) {
        av_packet_rescale_ts(p,w->encoder->time_base,w->stream->time_base); p->stream_index=w->stream->index;
        ret=av_interleaved_write_frame(w->format,p); av_packet_unref(p); if(ret<0)break;
        if(w->initial_vorbis_packets && --w->initial_vorbis_packets==0) {
            ret=av_opt_set_int(w->format->priv_data,"page_duration",w->ogg_page_duration,0);
            if(ret<0)break;
        }
    }
    av_packet_free(&p);
    return ret==AVERROR(EAGAIN)||ret==AVERROR_EOF ? 0 : failure(error,"write packet",ret);
}
static int drain(SaWriter *w,int flush,char *error) {
    int size=w->encoder->frame_size;
    while(av_audio_fifo_size(w->fifo)>0) {
        int n=av_audio_fifo_size(w->fifo);
        if(size && n<size && !flush)return 0;
        if(size && n>size)n=size;
        int allocated=n;
        if(size && n<size && !(w->encoder->codec->capabilities & (AV_CODEC_CAP_SMALL_LAST_FRAME|AV_CODEC_CAP_VARIABLE_FRAME_SIZE)))allocated=size;
        AVFrame *f=frame_alloc(w->encoder->sample_fmt,&w->encoder->ch_layout,w->encoder->sample_rate,allocated);
        if(!f)return failure(error,"allocate encoder frame",AVERROR(ENOMEM));
        av_samples_set_silence(f->extended_data,0,allocated,w->encoder->ch_layout.nb_channels,w->encoder->sample_fmt);
        int ret=av_audio_fifo_read(w->fifo,(void **)f->extended_data,n);
        if(ret!=n){av_frame_free(&f);return failure(error,"read FIFO",AVERROR(EIO));}
        if(allocated>n)w->encoder->trailing_padding=allocated-n;
        if(allocated>INT64_MAX-w->pts){av_frame_free(&f);return failure(error,"output PTS overflow",AVERROR(EOVERFLOW));}
        f->pts=w->pts; w->pts+=allocated;
        ret=encode(w,f,error); av_frame_free(&f); if(ret<0)return ret;
    }
    return 0;
}
static int resample(SaWriter *w,const uint8_t **input,int count,char *error) {
    int64_t capacity=av_rescale_rnd(swr_get_delay(w->swr,w->source_rate)+count,w->encoder->sample_rate,w->source_rate,AV_ROUND_UP);
    if(capacity>INT_MAX)return failure(error,"resample size",AVERROR(EOVERFLOW));
    if(capacity<=0)capacity=32;
    AVFrame *f=frame_alloc(w->encoder->sample_fmt,&w->encoder->ch_layout,w->encoder->sample_rate,(int)capacity);
    if(!f)return failure(error,"allocate resampler frame",AVERROR(ENOMEM));
    int n=swr_convert(w->swr,f->extended_data,(int)capacity,input,count);
    if(n>0 && w->final_frames>=0 && n>w->final_frames-w->valid_frames)
        n=(int)(w->final_frames-w->valid_frames);
    int ret=n;
    if(n>0) {
        ret=av_audio_fifo_realloc(w->fifo,av_audio_fifo_size(w->fifo)+n);
        if(ret>=0)ret=av_audio_fifo_write(w->fifo,(void **)f->extended_data,n)==n ? 0 : AVERROR(EIO);
        if(n>INT64_MAX-w->valid_frames)ret=AVERROR(EOVERFLOW);
        else w->valid_frames+=n;
    }
    av_frame_free(&f); if(ret<0)return failure(error,"resample/FIFO",ret);
    ret=drain(w,0,error); return ret<0?ret:n;
}
void sa_writer_free(SaWriter *w) {
    if(!w)return;
    swr_free(&w->swr); if(w->fifo)av_audio_fifo_free(w->fifo);
    avcodec_free_context(&w->encoder);
    if(w->format) { if(w->format->pb)avio_closep(&w->format->pb); avformat_free_context(w->format); }
    av_free(w);
}
SaWriter *sa_writer_open(SaReader *r,const char *path,const char *muxer,const char *codec,
    int rate,int channels,int64_t bitrate,const char *requested,int threads,char *error) {
    SaWriter *w=av_mallocz(sizeof(*w)); int ret=AVERROR(ENOMEM);
    if(!w){failure(error,"allocate writer",ret);return NULL;}
    w->reader=r; w->source_rate=r->rate; w->final_frames=-1;
    ret=avformat_alloc_output_context2(&w->format,NULL,muxer,path); if(ret<0)goto fail;
    const AVCodec *c=avcodec_find_encoder_by_name(codec); if(!c){ret=AVERROR_ENCODER_NOT_FOUND;goto fail;}
    w->encoder=avcodec_alloc_context3(c); if(!w->encoder){ret=AVERROR(ENOMEM);goto fail;}
    const enum AVSampleFormat *formats=NULL; int format_count=0;
    ret=avcodec_get_supported_config(w->encoder,c,AV_CODEC_CONFIG_SAMPLE_FORMAT,0,(const void **)&formats,&format_count);
    if(ret<0)goto fail;
    enum AVSampleFormat fmt=AV_SAMPLE_FMT_NONE;
    if(requested && *requested) {
        fmt=av_get_sample_fmt(requested); int found=!formats;
        for(int i=0;i<format_count;i++)if(formats[i]==fmt)found=1;
        if(fmt==AV_SAMPLE_FMT_NONE || !found){ret=AVERROR(EINVAL);goto fail;}
    } else fmt=formats&&format_count?formats[0]:AV_SAMPLE_FMT_S16;
    w->encoder->sample_fmt=fmt; w->encoder->sample_rate=rate; w->encoder->time_base=(AVRational){1,rate};
    w->encoder->bit_rate=bitrate; w->encoder->thread_count=threads;
    /* Explicit rates are validated in Rust. Give an unspecified AMR rate a
       legal default too, so metadata never describes the encoder's fallback. */
    if(!bitrate && c->id==AV_CODEC_ID_AMR_NB)w->encoder->bit_rate=12200;
    if(!bitrate && c->id==AV_CODEC_ID_AMR_WB)w->encoder->bit_rate=23850;
    w->encoder->log_level_offset=128;
    av_channel_layout_default(&w->encoder->ch_layout,channels);
    if(w->format->oformat->flags & AVFMT_GLOBALHEADER)w->encoder->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;
    ret=avcodec_open2(w->encoder,c,NULL); if(ret<0)goto fail;
    w->stream=avformat_new_stream(w->format,NULL); if(!w->stream){ret=AVERROR(ENOMEM);goto fail;}
    w->stream->time_base=w->encoder->time_base;
    ret=avcodec_parameters_from_context(w->stream->codecpar,w->encoder); if(ret<0)goto fail;
    ret=swr_alloc_set_opts2(&w->swr,&w->encoder->ch_layout,fmt,rate,&r->layout,r->sample_format,r->rate,0,NULL);
    if(ret<0)goto fail;
    ret=swr_init(w->swr); if(ret<0)goto fail;
    w->fifo=av_audio_fifo_alloc(fmt,channels,1024); if(!w->fifo){ret=AVERROR(ENOMEM);goto fail;}
    /* The WMA decoder skips two transform blocks, while the encoder declares
       only one. Prime the extra block so that decoding retains source frame 0. */
    if(c->id==AV_CODEC_ID_WMAV1 || c->id==AV_CODEC_ID_WMAV2) {
        int prime=w->encoder->initial_padding;
        AVFrame *silence=frame_alloc(fmt,&w->encoder->ch_layout,rate,prime);
        if(!silence){ret=AVERROR(ENOMEM);goto fail;}
        av_samples_set_silence(silence->extended_data,0,prime,channels,fmt);
        ret=av_audio_fifo_write(w->fifo,(void **)silence->extended_data,prime);
        av_frame_free(&silence);
        if(ret!=prime){if(ret>=0)ret=AVERROR(EIO);goto fail;}
    }
    w->format->interrupt_callback=(AVIOInterruptCB){interrupt,r};
    ret=avio_open2(&w->format->pb,path,AVIO_FLAG_WRITE,&w->format->interrupt_callback,NULL); if(ret<0)goto fail;
    /* A Vorbis stream fitting on one EOS page has no earlier granule from which
       demuxers can recover its initial overlap. Keep an early audio page. */
    if(c->id==AV_CODEC_ID_VORBIS && !strcmp(muxer,"ogg")) {
        ret=av_opt_get_int(w->format->priv_data,"page_duration",0,&w->ogg_page_duration); if(ret<0)goto fail;
        ret=av_opt_set_int(w->format->priv_data,"page_duration",1,0); if(ret<0)goto fail;
        w->initial_vorbis_packets=2;
    }
    ret=avformat_write_header(w->format,NULL); if(ret<0)goto fail;
    return w;
fail:
    failure(error,"open output (check container/codec/audio parameters)",ret); sa_writer_free(w); return NULL;
}
int sa_write(SaWriter *w,SaReader *r,int offset,int count,char *error) {
    if(offset<0 || count<0 || offset>r->frame->nb_samples-count)return failure(error,"sample selection",AVERROR(EINVAL));
    if(interrupt(r))return failure(error,"cancelled",AVERROR_EXIT);
    if(!count)return 0;
    if(count>INT64_MAX-w->source_frames)return failure(error,"selected source size",AVERROR(EOVERFLOW));
    w->source_frames+=count;
    AVFrame *selected=frame_alloc(r->sample_format,&r->layout,r->rate,count);
    if(!selected)return failure(error,"allocate selected frame",AVERROR(ENOMEM));
    int ret=av_samples_copy(selected->extended_data,r->frame->extended_data,0,offset,count,r->layout.nb_channels,r->sample_format);
    if(ret>=0)ret=resample(w,(const uint8_t **)selected->extended_data,count,error);
    av_frame_free(&selected); return ret<0?ret:0;
}
int sa_finish(SaWriter *w,SaInfo *info,char *error) {
    int ret;
    if(w->finished)return failure(error,"already finished",AVERROR(EINVAL));
    if(!w->source_frames)return failure(error,"empty audio selection",AVERROR_INVALIDDATA);
    w->final_frames=av_rescale_rnd(w->source_frames,w->encoder->sample_rate,w->source_rate,AV_ROUND_UP);
    if(w->final_frames<=0 || w->valid_frames>w->final_frames)
        return failure(error,"resampled duration overflow",AVERROR(EOVERFLOW));
    /* swr's EOF reflection is insufficient for inputs shorter than its initial
       filter window. Feed bounded zero extension, then keep only the source's
       duration on the output sample grid. Padding is never counted as content.
       Our default 32-tap filter (0.97 cutoff) fits within this 64-tap margin. */
    if(w->valid_frames<w->final_frames) {
        int64_t padding=av_rescale_rnd(64,w->source_rate,
            w->encoder->sample_rate<w->source_rate?w->encoder->sample_rate:w->source_rate,AV_ROUND_UP);
        if(padding>INT_MAX)return failure(error,"resampler padding",AVERROR(EOVERFLOW));
        ret=swr_inject_silence(w->swr,(int)padding);
        if(ret<0)return failure(error,"extend resampler tail",ret);
    }
    do { if(interrupt(w->reader))return failure(error,"cancelled",AVERROR_EXIT); ret=resample(w,NULL,0,error); } while(ret>0);
    if(ret<0)return ret;
    if(w->valid_frames!=w->final_frames)return failure(error,"incomplete resampler tail",AVERROR_INVALIDDATA);
    /* Encoders without delayed flush keep a lookahead window in their last
       frame. Speex's flush only closes a packet; it does not feed that window.
       Supply silence through the FIFO without counting it as source content. */
    int lookahead=sa_encoder_delay(w);
    if(w->valid_frames>0 && lookahead>0 &&
       (!(w->encoder->codec->capabilities & AV_CODEC_CAP_DELAY) ||
        w->encoder->codec_id==AV_CODEC_ID_SPEEX)) {
        AVFrame *tail=frame_alloc(w->encoder->sample_fmt,&w->encoder->ch_layout,w->encoder->sample_rate,lookahead);
        if(!tail)return failure(error,"allocate encoder lookahead",AVERROR(ENOMEM));
        av_samples_set_silence(tail->extended_data,0,lookahead,w->encoder->ch_layout.nb_channels,w->encoder->sample_fmt);
        ret=av_audio_fifo_write(w->fifo,(void **)tail->extended_data,lookahead);
        av_frame_free(&tail);
        if(ret!=lookahead)return failure(error,"flush encoder lookahead",ret<0?ret:AVERROR(EIO));
    }
    ret=drain(w,1,error); if(ret<0)return ret;
    ret=encode(w,NULL,error); if(ret<0)return ret;
    w->stream->codecpar->trailing_padding=w->encoder->trailing_padding;
    ret=av_write_trailer(w->format); if(ret<0)return failure(error,"write trailer",ret);
    ret=avio_closep(&w->format->pb); if(ret<0)return failure(error,"close output",ret);
    memset(info,0,sizeof(*info)); info->rate=w->encoder->sample_rate; info->channels=w->encoder->ch_layout.nb_channels;
    info->frames=w->valid_frames;
    info->bitrate=w->encoder->bit_rate;
    info->bits_per_sample=w->encoder->bits_per_raw_sample ? w->encoder->bits_per_raw_sample : av_get_bits_per_sample(w->encoder->codec_id);
    snprintf(info->codec,sizeof(info->codec),"%s",w->encoder->codec->name);
    snprintf(info->sample_format,sizeof(info->sample_format),"%s",av_get_sample_fmt_name(w->encoder->sample_fmt));
    w->finished=1; return 0;
}
