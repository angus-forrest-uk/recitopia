import { AlertCircle, FileImage, Upload, WandSparkles } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { createImageRecipeImport } from "@/lib/api";
import type { Author, Cookbook, RecipeImport } from "@/lib/schema";

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

function toIntOrNull(value: string): number | null {
  if (value.trim().length === 0) {
    return null;
  }
  const parsed = Number(value);
  return Number.isNaN(parsed) ? null : Math.round(parsed);
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => {
      if (typeof reader.result === "string") {
        resolve(reader.result);
      } else {
        reject(new Error("Could not read image"));
      }
    });
    reader.addEventListener("error", () =>
      reject(reader.error ?? new Error("Could not read image")),
    );
    reader.readAsDataURL(file);
  });
}

interface RecipeImportPanelProps {
  cookbooks: Cookbook[];
  authors: Author[];
  onUseDraft: (recipeImport: RecipeImport) => void;
}

export function RecipeImportPanel({ cookbooks, authors, onUseDraft }: RecipeImportPanelProps) {
  const [file, setFile] = useState<File | null>(null);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [cookbookId, setCookbookId] = useState(cookbooks[0]?.id ?? "");
  const [pageStart, setPageStart] = useState("");
  const [pageEnd, setPageEnd] = useState("");
  const [sourceLabel, setSourceLabel] = useState("");
  const [selectedAuthorIds, setSelectedAuthorIds] = useState<string[]>([]);
  const [recipeImport, setRecipeImport] = useState<RecipeImport | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedCookbook = useMemo(
    () => cookbooks.find((cookbook) => cookbook.id === cookbookId) ?? null,
    [cookbooks, cookbookId],
  );

  async function handleFileChange(nextFile: File | null) {
    setFile(nextFile);
    setRecipeImport(null);
    setError(null);
    if (!nextFile) {
      setPreviewUrl(null);
      return;
    }
    setPreviewUrl(await readFileAsDataUrl(nextFile));
    if (!sourceLabel.trim() && selectedCookbook) {
      setSourceLabel(selectedCookbook.title);
    }
  }

  function toggleAuthor(authorId: string) {
    setSelectedAuthorIds((current) =>
      current.includes(authorId)
        ? current.filter((value) => value !== authorId)
        : [...current, authorId],
    );
  }

  async function handleImport() {
    if (!file || !previewUrl || !cookbookId) {
      setError("Choose a photo and cookbook first.");
      return;
    }

    setError(null);
    setIsImporting(true);
    const result = await createImageRecipeImport({
      fileName: file.name,
      mimeType: file.type || "image/jpeg",
      imageBase64: previewUrl,
      cookbookId,
      authorIds: selectedAuthorIds,
      pageStart: toIntOrNull(pageStart),
      pageEnd: toIntOrNull(pageEnd),
      sourceLabel: sourceLabel.trim() || selectedCookbook?.title || null,
    });
    setIsImporting(false);

    if (!result.ok) {
      setError(result.error);
      return;
    }
    setRecipeImport(result.recipeImport);
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <FileImage className="h-4 w-4" aria-hidden="true" />
          Import from photo
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Input
          aria-label="Recipe photo"
          type="file"
          accept="image/*"
          onChange={(event) => void handleFileChange(event.target.files?.[0] ?? null)}
        />

        {previewUrl ? (
          <img
            src={previewUrl}
            alt="Selected recipe page"
            className="h-40 w-full rounded-md border object-cover"
          />
        ) : null}

        <label className="space-y-1 text-sm" htmlFor="import-cookbook">
          <span>Cookbook</span>
          <select
            id="import-cookbook"
            className={selectClassName}
            value={cookbookId}
            onChange={(event) => setCookbookId(event.target.value)}
          >
            {cookbooks.map((cookbook) => (
              <option key={cookbook.id} value={cookbook.id}>
                {cookbook.title}
              </option>
            ))}
          </select>
        </label>

        <div className="grid grid-cols-2 gap-2">
          <Input
            aria-label="Page start"
            type="number"
            min="1"
            placeholder="Page start"
            value={pageStart}
            onChange={(event) => setPageStart(event.target.value)}
          />
          <Input
            aria-label="Page end"
            type="number"
            min="1"
            placeholder="Page end"
            value={pageEnd}
            onChange={(event) => setPageEnd(event.target.value)}
          />
        </div>

        <Input
          aria-label="Source label"
          placeholder="Source label"
          value={sourceLabel}
          onChange={(event) => setSourceLabel(event.target.value)}
        />

        {authors.length > 0 ? (
          <div className="flex flex-wrap gap-2">
            {authors.map((author) => (
              <Button
                key={author.id}
                type="button"
                size="sm"
                variant={selectedAuthorIds.includes(author.id) ? "default" : "outline"}
                onClick={() => toggleAuthor(author.id)}
              >
                {author.name}
              </Button>
            ))}
          </div>
        ) : null}

        <Button type="button" className="w-full" disabled={isImporting} onClick={handleImport}>
          {isImporting ? (
            <WandSparkles className="h-4 w-4 animate-pulse" aria-hidden="true" />
          ) : (
            <Upload className="h-4 w-4" aria-hidden="true" />
          )}
          {isImporting ? "Importing" : "Run import"}
        </Button>

        {error ? (
          <p className="flex items-center gap-2 text-sm text-destructive">
            <AlertCircle className="h-4 w-4" aria-hidden="true" />
            {error}
          </p>
        ) : null}

        {recipeImport ? (
          <div className="space-y-3 rounded-md border p-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge>{recipeImport.status.replace("_", " ")}</Badge>
              <Badge variant="outline">{recipeImport.ocrEngine}</Badge>
            </div>
            {recipeImport.validationIssues.length > 0 ? (
              <ul className="space-y-1 text-sm text-muted-foreground">
                {recipeImport.validationIssues.slice(0, 4).map((issue) => (
                  <li key={`${issue.field}-${issue.severity}-${issue.message}`}>{issue.message}</li>
                ))}
              </ul>
            ) : null}
            <textarea
              className="h-28 w-full rounded-md border bg-background p-2 text-sm"
              readOnly
              value={recipeImport.ocrText || "No OCR text was returned."}
            />
            <Button
              type="button"
              variant="outline"
              className="w-full"
              disabled={!recipeImport.draft}
              onClick={() => onUseDraft(recipeImport)}
            >
              <WandSparkles className="h-4 w-4" aria-hidden="true" />
              Open draft
            </Button>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
