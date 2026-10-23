/* DocConvert.dll — document conversion behind one clean C ABI:
 * Markdown, HTML, CSV, JSON, and plain text. All strings UTF-8,
 * NUL-terminated. char* results are caller-owned: free with
 * dc_free_string(). Format names: "markdown"/"md", "html",
 * "csv", "json", "text"/"txt". The supported conversion pairs are
 * enumerable at runtime with dc_matrix_json(). */
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

DC_API char *dc_formats_json(void); /* ["markdown","html","csv","json","text"] */
DC_API char *dc_matrix_json(void);  /* [{"from":...,"to":...}, ...] */
DC_API char *dc_convert(const char *input, const char *from_format, const char *to_format);
DC_API int dc_convert_file(const char *in_path, const char *out_path,
                           const char *from_format, const char *to_format); /* NULL format = by extension */

#ifdef __cplusplus
}
#endif
#endif
