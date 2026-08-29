import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CookbookOverview } from "@/components/CookbookOverview";
import { seedCatalogue } from "@/data/seed";

describe("CookbookOverview", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("submits cookbook image-set imports from the selected cookbook view", async () => {
    const refresh = vi.fn(async () => {});
    const archiveImportResponseText = JSON.stringify({
      importRecord: {
        id: "cookbook-import-test",
        cookbookId: "one-pot-pan-planet",
        sourceKind: "image_set",
        sourcePath: "imports/one-pot-pan-planet",
        status: "uploaded",
        ocrEngine: null,
        createdAt: "2026-07-07T12:00:00.000Z",
        updatedAt: "2026-07-07T12:00:00.000Z",
        reviewNotes: "Browser image-set import: 1 files",
      },
      pageCount: 1,
      sectionCount: 0,
      contentBlockCount: 0,
      menuCount: 0,
      glossaryEntryCount: 0,
      supplierCount: 0,
      indexEntryCount: 0,
      crossReferenceCount: 0,
    });
    class MockXMLHttpRequest {
      static instances: MockXMLHttpRequest[] = [];

      body: XMLHttpRequestBodyInit | null = null;
      headers = new Map<string, string>();
      method = "";
      onerror: (() => void) | null = null;
      onload: (() => void) | null = null;
      responseText = archiveImportResponseText;
      status = 200;
      upload = {
        onload: null as (() => void) | null,
        onprogress: null as ((event: ProgressEvent) => void) | null,
      };
      url = "";

      constructor() {
        MockXMLHttpRequest.instances.push(this);
      }

      open(method: string, url: string) {
        this.method = method;
        this.url = url;
      }

      setRequestHeader(name: string, value: string) {
        this.headers.set(name, value);
      }

      send(body: XMLHttpRequestBodyInit) {
        this.body = body;
        const size = body instanceof Blob ? body.size : 0;
        this.upload.onprogress?.({
          lengthComputable: true,
          loaded: Math.floor(size / 2),
          total: size,
        } as ProgressEvent);
        this.upload.onprogress?.({
          lengthComputable: true,
          loaded: size,
          total: size,
        } as ProgressEvent);
        this.upload.onload?.();
        this.onload?.();
      }
    }
    vi.stubGlobal("XMLHttpRequest", MockXMLHttpRequest);

    render(
      <CookbookOverview
        catalogue={seedCatalogue}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
        onImportComplete={refresh}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    const imageInput = screen.getByLabelText("Images") as HTMLInputElement;
    const file = new File(["placeholder"], "our-korean-kitchen-128.png", {
      type: "image/png",
    });
    fireEvent.change(imageInput, { target: { files: [file] } });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() => expect(MockXMLHttpRequest.instances).toHaveLength(1));
    const request = MockXMLHttpRequest.instances[0];
    const requestUrl = new URL(request.url, "http://localhost");
    expect(request.method).toBe("POST");
    expect(requestUrl.pathname).toBe("/api/cookbook-imports/archive");
    expect(requestUrl.searchParams.get("cookbookId")).toBe("one-pot-pan-planet");
    expect(requestUrl.searchParams.get("sourcePath")).toBe("imports/one-pot-pan-planet");
    expect(request.headers.get("content-type")).toBe("application/x-tar");
    expect(request.body).toBeInstanceOf(Blob);
    expect((request.body as Blob).size).toBeGreaterThan(1024);
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Imported 1 pages as uploaded.")).toBeInTheDocument();
  });

  it("shows the archive upload response body when the server returns a non-JSON error", async () => {
    const refresh = vi.fn(async () => {});
    class MockXMLHttpRequest {
      static instances: MockXMLHttpRequest[] = [];

      body: XMLHttpRequestBodyInit | null = null;
      headers = new Map<string, string>();
      method = "";
      onerror: (() => void) | null = null;
      onload: (() => void) | null = null;
      responseText = "tar: short read";
      status = 400;
      upload = {
        onload: null as (() => void) | null,
        onprogress: null as ((event: ProgressEvent) => void) | null,
      };
      url = "";

      constructor() {
        MockXMLHttpRequest.instances.push(this);
      }

      open(method: string, url: string) {
        this.method = method;
        this.url = url;
      }

      setRequestHeader(name: string, value: string) {
        this.headers.set(name, value);
      }

      send(body: XMLHttpRequestBodyInit) {
        this.body = body;
        const size = body instanceof Blob ? body.size : 0;
        this.upload.onprogress?.({
          lengthComputable: true,
          loaded: size,
          total: size,
        } as ProgressEvent);
        this.upload.onload?.();
        this.onload?.();
      }
    }
    vi.stubGlobal("XMLHttpRequest", MockXMLHttpRequest);

    render(
      <CookbookOverview
        catalogue={seedCatalogue}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
        onImportComplete={refresh}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    const imageInput = screen.getByLabelText("Images") as HTMLInputElement;
    const file = new File(["placeholder"], "our-korean-kitchen-129.jpg", {
      type: "image/jpeg",
    });
    fireEvent.change(imageInput, { target: { files: [file] } });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() => expect(MockXMLHttpRequest.instances).toHaveLength(1));
    expect(await screen.findByText("Request failed with 400: tar: short read")).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("processes OCR for an uploaded cookbook import", async () => {
    const refresh = vi.fn(async () => {});
    const fetchMock = vi.fn(async (url: string | URL | Request, _init?: RequestInit) => {
      if (String(url).endsWith("/progress")) {
        return new Response(JSON.stringify({ error: "not found" }), {
          headers: { "content-type": "application/json" },
          status: 404,
        });
      }

      return new Response(
        JSON.stringify({
          importId: "cookbook-import-uploaded",
          state: "complete",
          stage: "complete",
          message: "Cookbook import processing complete.",
          current: 1,
          total: 1,
          processedCount: 1,
          skippedCount: 0,
          failedCount: 0,
          sectionCount: 1,
          contentBlockCount: 1,
          recipeCount: 1,
          extractionEngine: "deepseek",
          currentSectionIndex: null,
          sectionTotal: null,
          currentSectionTitle: null,
          error: null,
        }),
        { headers: { "content-type": "application/json" }, status: 202 },
      );
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-uploaded",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "uploaded",
              ocrEngine: null,
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:00:00.000Z",
              reviewNotes: null,
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-uploaded-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-uploaded",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
              ocrText: "",
              ocrJson: "{}",
              hasOcrText: false,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "unknown",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
        onImportComplete={refresh}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    fireEvent.click(screen.getByRole("button", { name: "Process OCR" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith("/api/cookbook-imports/cookbook-import-uploaded/ocr", {
        method: "POST",
      }),
    );
    expect(
      await screen.findByText(
        "OCR processed 1 pages, skipped 0, failed 0. Mapped 1 sections, 1 context blocks, and 1 recipes.",
      ),
    ).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("regenerates cookbook OCR and extraction from source images", async () => {
    const refresh = vi.fn(async () => {});
    const fetchMock = vi.fn(async (url: string | URL | Request, _init?: RequestInit) => {
      if (String(url).endsWith("/progress")) {
        return new Response(JSON.stringify({ error: "not found" }), {
          headers: { "content-type": "application/json" },
          status: 404,
        });
      }

      return new Response(
        JSON.stringify({
          importId: "cookbook-import-mapped",
          state: "complete",
          stage: "complete",
          message: "Cookbook import processing complete.",
          current: 1,
          total: 1,
          processedCount: 1,
          skippedCount: 0,
          failedCount: 0,
          sectionCount: 2,
          contentBlockCount: 3,
          recipeCount: 2,
          extractionEngine: "deepseek",
          currentSectionIndex: null,
          sectionTotal: null,
          currentSectionTitle: null,
          error: null,
        }),
        { headers: { "content-type": "application/json" }, status: 202 },
      );
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-mapped",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "mapped",
              ocrEngine: "paddleocr:paddle",
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:05:00.000Z",
              reviewNotes: "OCR process: 1 processed, 0 skipped, 0 failed.",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-mapped-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-mapped",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
              ocrText: "Kimchi Stew\nIngredients\nMethod",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "recipe",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
        onImportComplete={refresh}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/cookbook-imports/cookbook-import-mapped/ocr?refreshOcr=true",
        {
          method: "POST",
        },
      ),
    );
    expect(
      await screen.findByText(
        "Regenerated 1 pages, skipped 0, failed 0. Mapped 2 sections, 3 context blocks, and 2 recipes.",
      ),
    ).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("cancels a running cookbook import job", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request, _init?: RequestInit) => {
      if (String(url).endsWith("/progress")) {
        return new Response(JSON.stringify({ error: "not found" }), {
          headers: { "content-type": "application/json" },
          status: 404,
        });
      }

      const isCancel = String(url) === "/api/cookbook-imports/cookbook-import-mapped/cancel";
      return new Response(
        JSON.stringify({
          importId: "cookbook-import-mapped",
          state: isCancel ? "canceled" : "running",
          stage: isCancel ? "canceled" : "ocr_pages",
          message: isCancel ? "Cancellation requested." : "Refreshing page OCR.",
          current: 0,
          total: 1,
          processedCount: 0,
          skippedCount: 0,
          failedCount: 0,
          sectionCount: 0,
          contentBlockCount: 0,
          recipeCount: 0,
          extractionEngine: null,
          currentSectionIndex: null,
          sectionTotal: null,
          currentSectionTitle: null,
          error: null,
        }),
        { headers: { "content-type": "application/json" }, status: isCancel ? 200 : 202 },
      );
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-mapped",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "mapped",
              ocrEngine: "paddleocr:paddle",
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:05:00.000Z",
              reviewNotes: "OCR process: 1 processed, 0 skipped, 0 failed.",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-mapped-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-mapped",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
              ocrText: "Kimchi Stew\nIngredients\nMethod",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "recipe",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/cookbook-imports/cookbook-import-mapped/cancel",
        {
          method: "POST",
        },
      ),
    );
    expect(await screen.findByText("Cookbook import processing canceled.")).toBeInTheDocument();
  });

  it("starts a cookbook pipeline diagnostic from the selected cookbook view", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      const requestUrl = String(url);
      if (requestUrl.startsWith("/api/pipeline-diagnostics/cookbook")) {
        expect(init?.method).toBe("POST");
        return new Response(
          JSON.stringify({
            importId: "diagnostic-test",
            state: "complete",
            stage: "complete",
            message: "Pipeline diagnostic complete.",
            current: 1,
            total: 1,
            processedCount: 1,
            skippedCount: 0,
            failedCount: 0,
            sectionCount: 2,
            contentBlockCount: 3,
            recipeCount: 2,
            extractionEngine: "deepseek",
            currentSectionIndex: null,
            sectionTotal: null,
            currentSectionTitle: null,
            error: null,
          }),
          { headers: { "content-type": "application/json" }, status: 202 },
        );
      }

      return new Response(JSON.stringify({ error: "not found" }), {
        headers: { "content-type": "application/json" },
        status: 404,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-diagnostic",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "ocr_ready",
              ocrEngine: "paddleocr:paddle",
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:05:00.000Z",
              reviewNotes: "OCR process: 1 processed, 0 skipped, 0 failed.",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-diagnostic-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-diagnostic",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "abababababababababababababababababababababababababababababababab",
              ocrText: "Lemon Rice\nIngredients\nMethod",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "recipe",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    fireEvent.click(screen.getByRole("button", { name: "Run pipeline check" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/pipeline-diagnostics/cookbook?cookbookId=one-pot-pan-planet",
        { method: "POST" },
      ),
    );
    expect(
      await screen.findByText(
        "Pipeline check complete. OCR 1 page, 2 sections, 3 text blocks, 2 recipes.",
      ),
    ).toBeInTheDocument();
  });

  it("runs the Our Korean Kitchen introduction page diagnostic from the selected cookbook view", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      const requestUrl = String(url);
      if (requestUrl.startsWith("/api/pipeline-diagnostics/introduction-page")) {
        expect(init?.method).toBe("POST");
        return new Response(
          JSON.stringify({
            importId: "diagnostic-intro-test",
            state: "complete",
            stage: "complete",
            message: "Introduction page diagnostic complete.",
            current: 1,
            total: 1,
            processedCount: 1,
            skippedCount: 0,
            failedCount: 0,
            sectionCount: 1,
            contentBlockCount: 1,
            recipeCount: 0,
            extractionEngine: "deepseek",
            currentSectionIndex: null,
            sectionTotal: null,
            currentSectionTitle: null,
            error: null,
          }),
          { headers: { "content-type": "application/json" }, status: 202 },
        );
      }
      if (requestUrl === "/api/pipeline-diagnostics/diagnostic-intro-test/introduction-page") {
        return new Response(
          JSON.stringify({
            jobId: "diagnostic-intro-test",
            cookbookId: "our-korean-kitchen",
            pageId: "okk-page-004",
            selectedBy: "image_index",
            imageIndex: 4,
            storedPrintedPageNumber: null,
            detectedPrintedPageNumber: 7,
            ocrEngine: "paddleocr:3.7.0:paddleocr3",
            ocrLayoutMode: "columns",
            ocrColumnDetection: "edge-alignment",
            extractionEngine: "deepseek",
            sourceMapSectionCount: 1,
            sourceMapContentBlockCount: 1,
            extractedRecipeCount: 0,
            extractedContentBlockCount: 1,
            checksPassed: false,
            issues: [
              "ocr_order:While I am still passionate:before:Years ago, when Jina first introduced",
            ],
            expectedOcrOrder: ["Our Korean kitchen is an unusual one"],
            ocrTextPreview: "Our Korean kitchen is an unusual one",
            extractedBlockPreview: "Our Korean kitchen is an unusual one",
            artifacts: {
              workDir:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page",
              ocrTextPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/01-paddle-ocr-text.txt",
              ocrOutputPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/01-paddle-ocr-output.json",
              sourceMapInputPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/02-source-map-input.json",
              sourceMapOutputPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/03-source-map-output.json",
              deepseekInputPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/04-deepseek-input.json",
              deepseekOutputPath:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/05-deepseek-output.json",
              deepseekVerboseDir:
                "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/deepseek/verbose",
            },
          }),
          { headers: { "content-type": "application/json" }, status: 200 },
        );
      }

      return new Response(JSON.stringify({ error: "not found" }), {
        headers: { "content-type": "application/json" },
        status: 404,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          authors: [
            ...seedCatalogue.authors,
            { id: "jordan-bourke", name: "Jordan Bourke", website: null },
            { id: "rejina-pyo", name: "Rejina Pyo", website: null },
          ],
          cookbooks: [
            ...seedCatalogue.cookbooks,
            {
              id: "our-korean-kitchen",
              title: "Our Korean Kitchen",
              authorIds: ["jordan-bourke", "rejina-pyo"],
              isbn: null,
              publisher: "Weidenfeld & Nicolson",
              publishedYear: null,
              coverImageUrl: null,
              ownerUserId: "avery-river",
              familyId: "river-house",
              shareScope: "family",
              sharedWithUserIds: [],
            },
          ],
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "okk-import",
              cookbookId: "our-korean-kitchen",
              sourceKind: "image_set",
              sourcePath: "imports/our-korean-kitchen",
              status: "ocr_ready",
              ocrEngine: "paddleocr:3.7.0:paddleocr3",
              createdAt: "2026-07-09T12:00:00.000Z",
              updatedAt: "2026-07-09T12:05:00.000Z",
              reviewNotes: "OCR ready",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "okk-page-004",
              cookbookId: "our-korean-kitchen",
              importId: "okk-import",
              imageIndex: 4,
              printedPageLabel: null,
              printedPageNumber: null,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/okk-004.jpg",
              imageHash: "abababababababababababababababababababababababababababababababab",
              ocrText: "Our Korean kitchen is an unusual one",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "essay",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("Our Korean Kitchen"));
    fireEvent.click(screen.getByRole("button", { name: "Run intro page check" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/pipeline-diagnostics/introduction-page?cookbookId=our-korean-kitchen&imageIndex=4&printedPage=7",
        { method: "POST" },
      ),
    );
    expect(await screen.findByText("Introduction page check found 1 issue.")).toBeInTheDocument();
    expect(await screen.findByText("Needs review")).toBeInTheDocument();
    expect(await screen.findByText("DeepSeek input")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/pipeline-diagnostics/diagnostic-intro-test/introduction-page",
    );
    expect(
      await screen.findByText(
        "/var/lib/recitopia/imports/diagnostics/diagnostic-intro-test/introduction-page/04-deepseek-input.json",
      ),
    ).toBeInTheDocument();
  });

  it("cancels a running cookbook pipeline diagnostic", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      const requestUrl = String(url);
      if (requestUrl.startsWith("/api/pipeline-diagnostics/cookbook")) {
        expect(init?.method).toBe("POST");
        return new Response(
          JSON.stringify({
            importId: "diagnostic-test",
            state: "running",
            stage: "deepseek_section",
            message: "Running DeepSeek extraction against real cookbook pages.",
            current: 1,
            total: 1,
            processedCount: 1,
            skippedCount: 0,
            failedCount: 0,
            sectionCount: 1,
            contentBlockCount: 1,
            recipeCount: 0,
            extractionEngine: null,
            currentSectionIndex: 1,
            sectionTotal: 1,
            currentSectionTitle: "Diagnostic mini cookbook",
            error: null,
          }),
          { headers: { "content-type": "application/json" }, status: 202 },
        );
      }
      if (requestUrl === "/api/pipeline-diagnostics/diagnostic-test/cancel") {
        expect(init?.method).toBe("POST");
        return new Response(
          JSON.stringify({
            importId: "diagnostic-test",
            state: "canceled",
            stage: "canceled",
            message: "Cancellation requested.",
            current: 1,
            total: 1,
            processedCount: 1,
            skippedCount: 0,
            failedCount: 0,
            sectionCount: 1,
            contentBlockCount: 1,
            recipeCount: 0,
            extractionEngine: null,
            currentSectionIndex: null,
            sectionTotal: 1,
            currentSectionTitle: null,
            error: null,
          }),
          { headers: { "content-type": "application/json" }, status: 200 },
        );
      }

      return new Response(JSON.stringify({ error: "not found" }), {
        headers: { "content-type": "application/json" },
        status: 404,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-diagnostic",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "ocr_ready",
              ocrEngine: "paddleocr:paddle",
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:05:00.000Z",
              reviewNotes: "OCR process: 1 processed, 0 skipped, 0 failed.",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-diagnostic-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-diagnostic",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "abababababababababababababababababababababababababababababababab",
              ocrText: "Lemon Rice\nIngredients\nMethod",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "recipe",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));
    fireEvent.click(screen.getByRole("button", { name: "Run pipeline check" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(await screen.findByText("Pipeline check canceled.")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith("/api/pipeline-diagnostics/diagnostic-test/cancel", {
      method: "POST",
    });
  });

  it("shows OCR text for cookbook pages before section mapping exists", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request) => {
      if (String(url).endsWith("/text")) {
        return new Response(
          JSON.stringify({
            id: "cookbook-import-ocr-ready-page-1",
            ocrText: "Kimchi Stew\nIngredients\nMethod",
            ocrJson: "{}",
          }),
          { headers: { "content-type": "application/json" }, status: 200 },
        );
      }
      return new Response(JSON.stringify([]), {
        headers: { "content-type": "application/json" },
        status: 200,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={{
          ...seedCatalogue,
          cookbookImports: [
            ...seedCatalogue.cookbookImports,
            {
              id: "cookbook-import-ocr-ready",
              cookbookId: "one-pot-pan-planet",
              sourceKind: "image_set",
              sourcePath: "imports/one-pot-pan-planet",
              status: "ocr_ready",
              ocrEngine: "paddleocr:paddle",
              createdAt: "2026-07-07T12:00:00.000Z",
              updatedAt: "2026-07-07T12:05:00.000Z",
              reviewNotes: "OCR process: 1 processed, 0 skipped, 0 failed.",
            },
          ],
          cookbookPages: [
            ...seedCatalogue.cookbookPages,
            {
              id: "cookbook-import-ocr-ready-page-1",
              cookbookId: "one-pot-pan-planet",
              importId: "cookbook-import-ocr-ready",
              imageIndex: 1,
              printedPageLabel: "1",
              printedPageNumber: 1,
              imagePath: "/var/lib/recitopia/imports/cookbook-images/page.jpg",
              imageHash: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
              ocrText: "Kimchi Stew\nIngredients\nMethod",
              ocrJson: "{}",
              hasOcrText: true,
              averageConfidence: null,
              minimumConfidence: null,
              pageKind: "recipe",
              reviewStatus: "pending",
            },
          ],
        }}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("One: Pot, Pan, Planet"));

    // Pages awaiting section mapping surface in the source review workspace.
    expect(screen.getByRole("option", { name: /Page 1/ })).toBeInTheDocument();
    expect(await screen.findByText(/Kimchi Stew/)).toBeInTheDocument();
  });

  it("renders the single-page cookbook document with the recipe embedded in its section", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request) => {
      const requestUrl = String(url);
      if (requestUrl === "/api/cookbooks/east/blocks") {
        return new Response(JSON.stringify(seedCatalogue.cookbookContentBlocks), {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      }
      if (requestUrl.endsWith("/text")) {
        return new Response(JSON.stringify({ id: "east-page-086", ocrText: "", ocrJson: "{}" }), {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      }
      return new Response(JSON.stringify({ error: "not found" }), {
        headers: { "content-type": "application/json" },
        status: 404,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookOverview
        catalogue={seedCatalogue}
        onCreateCookbook={async (cookbook) => ({ ok: true, cookbook })}
      />,
    );

    fireEvent.click(screen.getByText("East"));

    expect(await screen.findByText("East — cookbook document")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Mains" })).toBeInTheDocument();

    // The extracted recipe is embedded in place of its source block: the
    // full components render and the raw block text is not duplicated.
    expect(screen.getByRole("heading", { name: "Tomato Coconut Dal" })).toBeInTheDocument();
    expect(screen.getByText("Ingredients")).toBeInTheDocument();
    expect(screen.getByText("Method")).toBeInTheDocument();
    expect(
      screen.getByText("Simmer lentils with tomatoes, coconut milk, water, and salt until soft."),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.queryByText("A preserved recipe block with source text, ingredients, and method."),
      ).not.toBeInTheDocument(),
    );
  });
});
