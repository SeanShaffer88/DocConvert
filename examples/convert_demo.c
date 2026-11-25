/* Minimal DocConvert client — shows the whole ABI in one file.
 *
 *   MSVC:  cl /Iinclude convert_demo.c docconvert.lib
 *   MinGW: gcc -Iinclude convert_demo.c -L. -ldocconvert
 *   (DocConvert.dll must sit next to the resulting executable.)
 *
 * Every char* the library returns is caller-owned: free it with
 * dc_free_string(). A NULL result is an error — read it from
 * dc_last_error() / dc_last_error_code().
 */
#include <stdio.h>

#include "docconvert.h"

static void print_or_report(char *result)
{
    if (result == NULL) {
        fprintf(stderr, "error %d: %s\n", dc_last_error_code(), dc_last_error());
        return;
    }
    printf("%s\n", result);
    dc_free_string(result);
}

int main(int argc, char **argv)
{
    const char *md =
        "# Hello\n\nSome **bold** text and a [link](https://example.com).\n";

    printf("DocConvert %s\n", dc_version());

    char *formats = dc_formats_json();
    printf("formats: %s\n", formats != NULL ? formats : "(none)");
    dc_free_string(formats);

    print_or_report(dc_convert(md, "md", "html"));

    /* Options are a JSON object; NULL means "defaults". */
    print_or_report(dc_convert_opts("---\ntitle: Demo\ndraft: false\n---\n\n# Body\n",
                                     "markdown", "html",
                                     "{\"front_matter\":\"table\"}"));

    /* The metadata is readable on its own, without converting. */
    print_or_report(dc_front_matter_json("---\ntitle: Demo\ndraft: false\n---\nBody\n"));

    /* Canonical Markdown, with the style layer opted into. */
    print_or_report(dc_normalize("_soft_ text\n\n```\ncode\n```\n",
                                 "{\"emphasis\":\"underscore\",\"fence\":\"tilde\"}"));

    /* A table of contents uses the same anchors as heading ids. */
    print_or_report(dc_toc_json("# Guide\n\n## Install\n\n## Run\n", "markdown"));

    /* Document syntaxes arrive as sources: rst becomes Markdown, then
     * the Markdown renderers finish the job. */
    print_or_report(dc_convert("Guide\n=====\n\n* one\n* two\n", "rst", "html"));

    /* A document's headings are its outline. */
    print_or_report(dc_convert("# Guide\n\n## Install\n", "markdown", "opml"));

    /* An unsupported pair is an error, never a lossy guess. */
    char *unsupported = dc_convert(md, "markdown", "csv");
    if (unsupported == NULL) {
        printf("markdown->csv: error %d: %s\n", dc_last_error_code(), dc_last_error());
    } else {
        dc_free_string(unsupported);
    }

    /* Files: NULL formats are detected from the extensions. */
    if (argc > 1) {
        int code = dc_convert_file(argv[1], "out.html", NULL, "html");
        printf("dc_convert_file(\"%s\") -> %d (%s)\n", argv[1], code,
               code == DC_OK ? "ok" : dc_last_error());
    } else {
        printf("pass a .md path to convert it to out.html\n");
    }
    return 0;
}
