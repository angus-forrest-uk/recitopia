import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CookbookSourceReview } from "@/components/CookbookSourceReview";
import { seedCatalogue } from "@/data/seed";
import type { CookbookPage } from "@/lib/schema";

function requireEastCookbook() {
  const cookbook = seedCatalogue.cookbooks.find((candidate) => candidate.id === "east");
  if (!cookbook) {
    throw new Error("seed catalogue is missing the east cookbook");
  }
  return cookbook;
}

const eastCookbook = requireEastCookbook();

const seededPage = seedCatalogue.cookbookPages[0];

function jsonResponse(payload: unknown, status = 200) {
  return new Response(JSON.stringify(payload), {
    headers: { "content-type": "application/json" },
    status,
  });
}

function renderReview(overrides?: {
  pages?: CookbookPage[];
  onPageUpdated?: () => Promise<void>;
  onUseDraft?: (recipeImport: unknown) => void;
}) {
  return render(
    <CookbookSourceReview
      cookbook={eastCookbook}
      pages={overrides?.pages ?? seedCatalogue.cookbookPages}
      recipesById={new Map(seedCatalogue.recipes.map((recipe) => [recipe.id, recipe] as const))}
      menus={seedCatalogue.cookbookMenus}
      glossaryEntries={seedCatalogue.cookbookGlossaryEntries}
      suppliers={seedCatalogue.cookbookSuppliers}
      indexEntries={seedCatalogue.cookbookIndexEntries}
      crossReferences={seedCatalogue.cookbookCrossReferences}
      onPageUpdated={overrides?.onPageUpdated}
      onUseDraft={overrides?.onUseDraft}
    />,
  );
}

describe("CookbookSourceReview", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads and shows full OCR text for the selected page", async () => {
    const fetchMock = vi.fn(async (url: string | URL | Request) => {
      expect(String(url)).toBe(`/api/cookbook-pages/${seededPage.id}/text`);
      return jsonResponse({
        id: seededPage.id,
        ocrText: "Tomato Coconut Dal\nFull page text beyond the preview",
        ocrJson: "{}",
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    renderReview();

    expect(await screen.findByText(/Full page text beyond the preview/)).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("patches the page review status and refreshes the catalogue", async () => {
    const refresh = vi.fn(async () => {});
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url).endsWith("/text")) {
        return jsonResponse({ id: seededPage.id, ocrText: "text", ocrJson: "{}" });
      }
      expect(String(url)).toBe(`/api/cookbook-pages/${seededPage.id}`);
      expect(init?.method).toBe("PATCH");
      expect(JSON.parse(String(init?.body))).toEqual({ reviewStatus: "accepted" });
      return jsonResponse({ ...seededPage, reviewStatus: "accepted" });
    });
    vi.stubGlobal("fetch", fetchMock);

    renderReview({ onPageUpdated: refresh });

    fireEvent.change(screen.getByLabelText("Review status"), {
      target: { value: "accepted" },
    });

    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
  });

  it("creates a recipe draft from the selected page and hands it off", async () => {
    const onUseDraft = vi.fn();
    const draftImport = {
      id: "import-draft-1",
      status: "draft_ready",
      fileName: "east-086.jpg",
      mimeType: "text/plain",
      imagePath: "cookbook-source:east-086",
      ocrEngine: "cookbook-source-text",
      ocrText: "Tomato Coconut Dal",
      ocrJson: "{}",
      draft: null,
      validationIssues: [],
      createdAt: "2026-07-08T12:00:00.000Z",
      updatedAt: "2026-07-08T12:00:00.000Z",
    };
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url).endsWith("/text")) {
        return jsonResponse({ id: seededPage.id, ocrText: "text", ocrJson: "{}" });
      }
      expect(String(url)).toBe("/api/cookbook-recipe-drafts");
      expect(JSON.parse(String(init?.body))).toEqual({
        cookbookId: "east",
        pageId: seededPage.id,
      });
      return jsonResponse(draftImport);
    });
    vi.stubGlobal("fetch", fetchMock);

    renderReview({ onUseDraft });

    fireEvent.click(screen.getByRole("button", { name: "Create recipe draft" }));

    expect(await screen.findByText(/Recipe draft created/)).toBeInTheDocument();
    expect(onUseDraft).toHaveBeenCalledTimes(1);
  });

  it("accepts a page's OCR text as non-recipe content", async () => {
    const refresh = vi.fn(async () => {});
    const acceptedBlock = {
      id: `${seededPage.id}-content`,
      cookbookId: "east",
      sectionId: "east-section-mains",
      pageStart: 86,
      pageEnd: 86,
      position: 2,
      kind: "paragraph",
      title: null,
      text: "Tomato Coconut Dal\nIngredients\nMethod",
      hasText: true,
      confidence: null,
      sourceJson: "{}",
    };
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url).endsWith("/text")) {
        return jsonResponse({ id: seededPage.id, ocrText: "text", ocrJson: "{}" });
      }
      expect(String(url)).toBe(`/api/cookbook-pages/${seededPage.id}/accept-content`);
      expect(init?.method).toBe("POST");
      return jsonResponse(acceptedBlock);
    });
    vi.stubGlobal("fetch", fetchMock);

    renderReview({ onPageUpdated: refresh });

    fireEvent.click(screen.getByRole("button", { name: "Accept as content" }));

    expect(await screen.findByText(/Page accepted as paragraph content/)).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("saves corrected OCR text for the selected page", async () => {
    const refresh = vi.fn(async () => {});
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url).endsWith("/text")) {
        return jsonResponse({ id: seededPage.id, ocrText: "Tomat0 Coconut DaI", ocrJson: "{}" });
      }
      expect(String(url)).toBe(`/api/cookbook-pages/${seededPage.id}`);
      expect(init?.method).toBe("PATCH");
      expect(JSON.parse(String(init?.body))).toEqual({ ocrText: "Tomato Coconut Dal" });
      return jsonResponse({ ...seededPage, ocrText: "Tomato Coconut Dal" });
    });
    vi.stubGlobal("fetch", fetchMock);

    renderReview({ onPageUpdated: refresh });

    const textarea = await screen.findByLabelText("OCR text");
    expect((textarea as HTMLTextAreaElement).value).toBe("Tomat0 Coconut DaI");

    fireEvent.change(textarea, { target: { value: "Tomato Coconut Dal" } });
    fireEvent.click(screen.getByRole("button", { name: "Save correction" }));

    expect(await screen.findByText(/OCR text saved/)).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(1);
    // The save controls disappear once the draft matches the saved text.
    expect(screen.queryByRole("button", { name: "Save correction" })).not.toBeInTheDocument();
  });

  it("renders source entities beyond count badges", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => jsonResponse({ id: seededPage.id, ocrText: "", ocrJson: "{}" })),
    );

    renderReview();

    expect(screen.getByText("Menus (1)")).toBeInTheDocument();
    expect(screen.getByText("Glossary (1)")).toBeInTheDocument();
    expect(screen.getByText("Suppliers (1)")).toBeInTheDocument();
    expect(screen.getByText("Index (1)")).toBeInTheDocument();
    // Menu recipes resolve to real recipe titles, not ids.
    expect(screen.getByText(/Tomato Coconut Dal — main/)).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("Loading page text…")).not.toBeInTheDocument());
  });
});
