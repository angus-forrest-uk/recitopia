import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { assembleCookbookDocument, CookbookDocumentView } from "@/components/CookbookDocumentView";
import { seedCatalogue } from "@/data/seed";
import type { CookbookContentBlock, CookbookSection, Recipe } from "@/lib/schema";

function requireRecipe(id: string): Recipe {
  const recipe = seedCatalogue.recipes.find((candidate) => candidate.id === id);
  if (!recipe) {
    throw new Error(`seed catalogue is missing recipe ${id}`);
  }
  return recipe;
}

const dal = requireRecipe("tomato-coconut-dal");
const noodles = requireRecipe("lemon-tahini-noodles");

const mains: CookbookSection = {
  id: "section-mains",
  cookbookId: "east",
  parentSectionId: null,
  title: "Mains",
  kind: "chapter",
  position: 2,
  pageStart: 80,
  pageEnd: 120,
};

const frontMatter: CookbookSection = {
  id: "section-front",
  cookbookId: "east",
  parentSectionId: null,
  title: "Introduction",
  kind: "front_matter",
  position: 1,
  pageStart: 1,
  pageEnd: 79,
};

function block(overrides: Partial<CookbookContentBlock> & { id: string }): CookbookContentBlock {
  return {
    cookbookId: "east",
    sectionId: null,
    pageStart: null,
    pageEnd: null,
    position: 1,
    kind: "paragraph",
    title: null,
    text: "",
    hasText: false,
    confidence: null,
    sourceJson: "{}",
    ...overrides,
  };
}

describe("assembleCookbookDocument", () => {
  it("orders sections by position and embeds a recipe in place of its source block", () => {
    const blocks = [
      block({
        id: "block-intro",
        sectionId: "section-front",
        pageStart: 4,
        position: 1,
        text: "Our kitchen is an unusual one.",
        hasText: true,
      }),
      block({
        id: "block-dal",
        sectionId: "section-mains",
        pageStart: 86,
        position: 2,
        kind: "recipe",
        title: "Tomato Coconut Dal",
        text: "Raw block text that the embedded recipe replaces.",
        hasText: true,
      }),
    ];

    const document = assembleCookbookDocument([mains, frontMatter], blocks, [dal]);

    expect(document.map((section) => section.title)).toEqual(["Introduction", "Mains"]);
    const mainsItems = document[1].items;
    expect(mainsItems).toHaveLength(1);
    expect(mainsItems[0]).toEqual({ kind: "recipe", recipe: dal });
  });

  it("places unanchored recipes into their section by page and orphans at the end", () => {
    const blocks = [
      block({
        id: "block-headnote",
        sectionId: "section-mains",
        pageStart: 84,
        position: 1,
        text: "A chapter of weeknight mains.",
        hasText: true,
      }),
    ];
    // dal has pageStart 86 (inside Mains); noodles has pageStart 112 but we
    // test the no-section-match fallback by narrowing the section range.
    const narrowMains = { ...mains, pageEnd: 100 };

    const document = assembleCookbookDocument([narrowMains], blocks, [dal, noodles]);

    expect(document.map((section) => section.title)).toEqual(["Mains", "More from this book"]);
    expect(document[0].items.map((item) => item.kind)).toEqual(["block", "recipe"]);
    expect(document[0].items[1]).toEqual({ kind: "recipe", recipe: dal });
    expect(document[1].items).toEqual([{ kind: "recipe", recipe: noodles }]);
  });

  it("shows recipes as a plain document when no source structure exists", () => {
    const document = assembleCookbookDocument([], [], [noodles]);

    expect(document).toHaveLength(1);
    expect(document[0].title).toBe("Recipes");
    expect(document[0].items).toEqual([{ kind: "recipe", recipe: noodles }]);
  });
});

describe("CookbookDocumentView block corrections", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("edits a content block's text and saves it through the API", async () => {
    const cookbook = seedCatalogue.cookbooks.find((candidate) => candidate.id === "east");
    if (!cookbook) {
      throw new Error("seed catalogue is missing the east cookbook");
    }
    const fullBlock = {
      ...seedCatalogue.cookbookContentBlocks[0],
      text: "Full block text from the blocks endpoint.",
      hasText: true,
    };
    const fetchMock = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url) === "/api/cookbooks/east/blocks") {
        return new Response(JSON.stringify([fullBlock]), {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      }
      expect(String(url)).toBe(`/api/cookbook-content-blocks/${fullBlock.id}`);
      expect(init?.method).toBe("PATCH");
      expect(JSON.parse(String(init?.body))).toEqual({ text: "Corrected block text." });
      return new Response(JSON.stringify({ ...fullBlock, text: "Corrected block text." }), {
        headers: { "content-type": "application/json" },
        status: 200,
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <CookbookDocumentView
        cookbook={cookbook}
        sections={seedCatalogue.cookbookSections}
        previewBlocks={seedCatalogue.cookbookContentBlocks}
        recipes={[]}
      />,
    );

    // Wait for full text so editing operates on complete content.
    expect(
      await screen.findByText("Full block text from the blocks endpoint."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));
    fireEvent.change(screen.getByLabelText(/Edit text of/), {
      target: { value: "Corrected block text." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save text" }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Corrected block text.")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Save text" })).not.toBeInTheDocument(),
    );
  });
});
