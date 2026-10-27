"""Qualify generated native R packages, shared wire behavior, and real HTTP."""
import os
from pathlib import Path
import tempfile
from sdk_test_support import ROOT, describe_sdk, run, snippets, weather
from sdk_cru_packages import replace_once
from sdk_native_docs_model import qualification_model
from sdk_http_fixture import fixture
from r_http_fixture import edge_fixture

R = os.environ.get("R", "R")
RSCRIPT = os.environ.get("RSCRIPT", "Rscript")

# Exact document, default and optional-long members, recursive through `next`.
EXACT_VALUES = """structure ExactValues {
    document: document
    defaultLong: long = 9223372036854775807
    optionalLong?: long
    next?: ExactValues
}
"""


# Parses a snippet, then checks that every function it calls by name, every
# condition class it handles and every client method it calls is defined
# once the package is attached: a misspelled constructor or method would
# otherwise surface only when a reader runs it.
SNIPPET_CHECK = r"""
args <- commandArgs(TRUE)
exprs <- parse(file = args[1L], keep.source = FALSE)
library(args[2L], character.only = TRUE, lib.loc = args[3L])
functions <- character(); methods <- character(); assigned <- character()
walk <- function(e) {
  if (is.call(e)) {
    head <- e[[1L]]
    if (is.name(head)) {
      name <- as.character(head)
      functions <<- c(functions, name)
      if (name == "<-" && is.name(e[[2L]])) assigned <<- c(assigned, as.character(e[[2L]]))
      if (name == "tryCatch") functions <<- c(functions, setdiff(names(e)[-1L], c("", "error", "finally")))
    } else if (is.call(head) && identical(head[[1L]], as.name("$")) && identical(head[[2L]], as.name("client"))) {
      methods <<- c(methods, as.character(head[[3L]]))
    }
    # An empty argument (`x[, 1]`) is the missing symbol, which no variable can hold.
    for (i in seq_along(e)) if (!identical(e[[i]], quote(expr = ))) walk(e[[i]])
  }
}
for (e in exprs) walk(e)
undefined <- Filter(function(name) !exists(name, mode = "function"), setdiff(unique(functions), assigned))
if (length(undefined)) stop("undefined functions: ", paste(undefined, collapse = ", "))
client_name <- args[4L]
client <- get(client_name)(client_config(endpoint = "https://example.invalid"))
absent <- Filter(function(name) !is.function(client[[name]]), unique(methods))
client$close()
if (length(absent)) stop("undefined client methods: ", paste(absent, collapse = ", "))
"""


def check_snippets(output):
    """The `crucible.sdks/1` snippets (docs/docs.md) parse and name what the
    package declares: each is described with the options its package is
    generated with, here a package name and no endpoint."""
    library = output / "snippet-library"
    library.mkdir(exist_ok=True)
    script = output / "snippet-check.R"
    script.write_text(SNIPPET_CHECK, encoding="utf-8", newline="\n")
    for model, package, client in (("weather", "weather.client", "WeatherClient"),
                                   ("samples", "samples.client", "SamplesClient")):
        generated = output / f"snippet-{model}"
        generate(ROOT / "tests/models" / model, generated, package)
        run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
        document = describe_sdk(f'[sdk.r]\nout = "generated/r"\npackage = "{package}"\n',
                                ROOT / "tests/models" / model)
        directory = output / "snippets" / model
        directory.mkdir(parents=True, exist_ok=True)
        count = 0
        for _, operation, variant, text in snippets(document):
            path = directory / f"{operation}.{variant}.R"
            path.write_text(text, encoding="utf-8", newline="\n")
            run(RSCRIPT, str(script), str(path), package, str(library), client)
            count += 1
        if count == 0:
            raise RuntimeError(f"no R snippets for {model}")


def generate(model, output, package="weather.client", *extra):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "r",
        str(model), "--out", str(output), "--package-name", package, *extra)



def qualify_error_docs(output):
    from sdk_error_docs_fixture import error_docs_fixture
    root = output / "error-docs"
    library = root / "library"
    library.mkdir(parents=True, exist_ok=True)
    package = root / "sdk"
    generate(ROOT / "tests/models/error-docs", package, "docs.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(package))
    with error_docs_fixture(root) as (endpoint, oracle):
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/error_docs.R"), str(library),
            env=dict(os.environ, ERROR_DOCS_URL=endpoint, ERROR_DOCS_CASES=str(oracle)))


def qualify(output):
    qualify_error_docs(output)
    package = output / "sdk"
    model_path = qualification_model(output)
    (model_path / "exact.cru").write_text(EXACT_VALUES, encoding="utf-8", newline="\n")
    replace_once(model_path, "        clientToken?: string\n",
                 "        clientToken?: string\n        exactValues?: ExactValues\n")
    generate(model_path, package)
    run(RSCRIPT, "-e", "options(warn=2); for (p in list.files(commandArgs(TRUE)[1], pattern=\"[.]Rd$\", full.names=TRUE)) tools::parse_Rd(p)", str(package / "man"))
    library = output / "library"
    library.mkdir(exist_ok=True)
    run(R, "CMD", "build", "--no-manual", "--no-build-vignettes", str(package), cwd=output)
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "weather.client_0.0.0.tar.gz"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/serialization.R"), str(library),
        env=dict(os.environ, SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
    with weather() as endpoint, fixture() as fixture_url:
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/runtime.R"), str(library),
            env=dict(os.environ, WEATHER_URL=endpoint, FIXTURE_URL=fixture_url,
                     RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
                     HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json")))
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/proxy.R"), str(library),
            env=dict(os.environ, http_proxy=fixture_url, https_proxy=fixture_url,
                     HTTP_PROXY=fixture_url, HTTPS_PROXY=fixture_url, NO_PROXY="", no_proxy=""))
    with edge_fixture() as endpoint:
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/transport_edges.R"), str(library),
            env=dict(os.environ, EDGE_URL=endpoint))
    run(RSCRIPT, "-e", "library(weather.client, lib.loc=commandArgs(TRUE)[1]); stopifnot(CreateCityInput(name='consumer')$name == 'consumer')", str(library))
    for name, model in [("empty", ROOT / "tests/models/empty-service")]:
        generated = output / name
        generate(model, generated, "empty.client")
        run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
        run(RSCRIPT, "-e", "library(empty.client, lib.loc=commandArgs(TRUE)[1]); EmptyClient(client_config(endpoint='http://127.0.0.1'))$close()", str(library))
    generated = output / "extended"
    generate(ROOT / "tests/models/extended-types", generated, "types.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/extensions.R"), str(library))
    # Typed permissions (docs/permissions.md §6.3), from the `.cru` fixture package.
    generate(ROOT / "tests/fixtures/permissions", output / "permissions", "leyline.client")
    run(RSCRIPT, "-e", "options(warn=2); for (p in list.files(commandArgs(TRUE)[1], pattern=\"[.]Rd$\", full.names=TRUE)) tools::parse_Rd(p)", str(output / "permissions" / "man"))
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "permissions"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/permissions.R"), str(library))
    generate(ROOT / "tests/models/error-payloads", output / "wire", "wire.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "wire"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/wire.R"), str(library))
    # Payload Content-Type precedence.
    generate(ROOT / "tests/models/payload-bindings", output / "payload", "payload.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "payload"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/payload_content_type.R"), str(library), "payload.client")
    check_snippets(output)
    print("R native package build/install, consumer and snippet qualification passed")


if __name__ == "__main__":
    explicit = os.environ.get("R_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        (qualify_error_docs if os.environ.get("ERROR_DOCS_ONLY") else qualify)(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-r-") as directory:
            (qualify_error_docs if os.environ.get("ERROR_DOCS_ONLY") else qualify)(Path(directory))
