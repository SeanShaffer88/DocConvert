/* DocConvert.dll v0.11.0 — document conversion behind one clean C ABI:
 * Markdown, HTML, CSV, TSV, JSON, and plain text. All strings UTF-8,
 * NUL-terminated. char* results are caller-owned: free with
 * dc_free_string(). Format names: "markdown"/"md", "html",
 * "csv", "tsv"/"tab", "json", "text"/"txt". The supported conversion pairs are
 * enumerable at runtime with dc_matrix_json().
 *
 * Options (dc_convert_opts / dc_convert_file_opts) are a JSON object;
 * NULL or "" means "use the defaults". Recognised keys:
 *   "front_matter": "strip" (default) | "table"
 *     What md->html and md->text do with a YAML front matter block:
 *     drop it, or render it as a key/value table.
 * Unknown keys or values fail with DC_ERR_INVALID_ARG rather than
 * being ignored.
 *
 * Two front ends with the same matrix ship alongside: docconvert.exe
 * ("docconvert capabilities" reports formats, pairs, options and exit
 * codes as one JSON object) and docconvert-mcp.exe, a Model Context
 * Protocol server for local AI clients.
 */
#ifndef DOCCONVERT_H
#define DOCCONVERT_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32) && defined(DC_BUILD)
#define DC_API __declspec(dllexport)
#elif defined(_WIN32)
#define DC_API __declspec(dllimport)
#else
#define DC_API
#endif
#define DC_OK 0
#define DC_ERR_INVALID_ARG 1
#define DC_ERR_NOT_SUPPORTED 2
#define DC_ERR_NOT_FOUND 3
#define DC_ERR_ACCESS_DENIED 4
#define DC_ERR_PLATFORM 5
#define DC_ERR_INTERNAL 6
/* Extension beyond the shared 0-6 set: input fails to parse. */
#define DC_ERR_CORRUPT 7

DC_API const char *dc_version(void);
DC_API int dc_version_major(void);
DC_API int dc_version_minor(void);
DC_API int dc_version_patch(void);
DC_API const char *dc_last_error(void);
DC_API int dc_last_error_code(void);
DC_API void dc_free_string(char *p);

DC_API char *dc_formats_json(void); /* ["markdown","html","csv","tsv","json","text"] */
DC_API char *dc_matrix_json(void);  /* [{"from":...,"to":...}, ...] */
DC_API char *dc_convert(const char *input, const char *from_format, const char *to_format);
DC_API char *dc_convert_opts(const char *input, const char *from_format,
                             const char *to_format, const char *options_json);
/* The YAML front matter of a Markdown document as a JSON object;
 * "{}" when there is none (or it is malformed). */
DC_API char *dc_front_matter_json(const char *input);
/* The headings of a Markdown or HTML document as
 * [{"level":2,"text":"...","anchor":"..."}] — the same anchors that
 * {"heading_ids":true} writes into Markdown->HTML output. */
DC_API char *dc_toc_json(const char *input, const char *from_format);
DC_API int dc_convert_file(const char *in_path, const char *out_path,
                           const char *from_format, const char *to_format); /* NULL format = by extension */
/* Canonical Markdown. options_json is optional and accepts
 * {"emphasis":"asterisk|underscore"} and {"fence":"backtick|tilde"};
 * unknown keys are DC_ERR_INVALID_ARG. */
DC_API char *dc_normalize(const char *input, const char *options_json);
/* Streams an identity conversion (markdown, html, text, rst, asciidoc and
 * opml copy the input byte for byte) in 64 KiB chunks, and streams text,
 * CSV/TSV and Markdown block by block; every other pair falls back to the
 * whole-buffer path. Returns the bytes written, or negative with
 * dc_last_error() set. */
DC_API long long dc_convert_file_streaming(const char *in_path, const char *out_path,
                                           const char *from_format, const char *to_format);
DC_API int dc_convert_file_opts(const char *in_path, const char *out_path,
                                const char *from_format, const char *to_format,
                                const char *options_json);

#ifdef __cplusplus
}
#endif
#endif
