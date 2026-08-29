import { Activity, CircleStop, FileText, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { CookbookDocumentView } from "@/components/CookbookDocumentView";
import { CookbookSourceReview } from "@/components/CookbookSourceReview";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  cancelCookbookImportProcessing,
  cancelPipelineDiagnostic,
  createCookbookArchiveImport,
  getCookbookImportProgress,
  getIntroductionPageDiagnostic,
  getPipelineDiagnosticProgress,
  type IntroductionPageDiagnostic,
  processCookbookImportOcr,
  runIntroductionPageDiagnostic,
  startCookbookPipelineDiagnostic,
} from "@/lib/api";
import type {
  Author,
  Catalogue,
  Cookbook,
  CookbookImport,
  CookbookImportProgress,
  CookbookPage,
  RecipeImport,
  ShareScope,
  User,
} from "@/lib/schema";

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

function authorNames(cookbook: Cookbook, authorsById: Map<string, Author>): string {
  return cookbook.authorIds.map((id) => authorsById.get(id)?.name ?? id).join(", ");
}

function userName(userId: string | null, usersById: Map<string, User>): string {
  if (!userId) {
    return "Unassigned";
  }
  return usersById.get(userId)?.displayName ?? userId;
}

function shareLabel(cookbook: Cookbook, usersById: Map<string, User>): string {
  if (cookbook.shareScope === "family") {
    return "Family";
  }
  if (cookbook.shareScope === "users") {
    const names = cookbook.sharedWithUserIds.map((id) => userName(id, usersById));
    return names.length > 0 ? names.join(", ") : "Selected users";
  }
  return "Personal";
}

function slugify(text: string): string {
  const slug = text
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug.length >= 2 ? slug : `${slug || "cookbook"}-cookbook`;
}

function toYearOrNull(value: string): number | null {
  if (value.trim().length === 0) {
    return null;
  }
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed >= 1400 && parsed <= 2600 ? parsed : null;
}

const tarBlockSize = 512;

function writeAscii(buffer: Uint8Array, offset: number, length: number, value: string) {
  const encoded = new TextEncoder().encode(value);
  buffer.set(encoded.slice(0, length), offset);
}

function writeOctal(buffer: Uint8Array, offset: number, length: number, value: number) {
  const octal = Math.floor(value)
    .toString(8)
    .padStart(length - 1, "0")
    .slice(-(length - 1));
  writeAscii(buffer, offset, length - 1, octal);
  buffer[offset + length - 1] = 0;
}

function tarSafeName(fileName: string) {
  const name = fileName.split(/[\\/]/).pop()?.trim() ?? "";
  return name.replace(/^\.+/, "").replaceAll("\0", "");
}

function tarHeader(file: File, safeName: string) {
  const header = new Uint8Array(tarBlockSize);
  writeAscii(header, 0, 100, safeName);
  writeOctal(header, 100, 8, 0o644);
  writeOctal(header, 108, 8, 0);
  writeOctal(header, 116, 8, 0);
  writeOctal(header, 124, 12, file.size);
  writeOctal(header, 136, 12, Math.floor((file.lastModified || Date.now()) / 1000));
  header.fill(32, 148, 156);
  header[156] = "0".charCodeAt(0);
  writeAscii(header, 257, 6, "ustar");
  writeAscii(header, 263, 2, "00");

  const checksum = header.reduce((total, byte) => total + byte, 0);
  const checksumText = checksum.toString(8).padStart(6, "0").slice(-6);
  writeAscii(header, 148, 6, checksumText);
  header[154] = 0;
  header[155] = 32;
  return header;
}

function buildTarArchive(files: File[]) {
  const parts: BlobPart[] = [];
  const names = new Set<string>();

  for (const file of files) {
    const safeName = tarSafeName(file.name);
    if (!safeName || safeName.length > 100 || names.has(safeName)) {
      throw new Error(`Cannot archive duplicate or unsupported filename: ${file.name}`);
    }
    names.add(safeName);
    parts.push(tarHeader(file, safeName), file);

    const padding = (tarBlockSize - (file.size % tarBlockSize)) % tarBlockSize;
    if (padding > 0) {
      parts.push(new Uint8Array(padding));
    }
  }

  parts.push(new Uint8Array(tarBlockSize * 2));
  return new Blob(parts, { type: "application/x-tar" });
}

function formatUploadBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  for (const unit of units) {
    if (value < 1024 || unit === units[units.length - 1]) {
      return `${value.toFixed(value >= 100 ? 0 : 1)} ${unit}`;
    }
    value /= 1024;
  }

  return `${bytes} B`;
}

interface CookbookOverviewProps {
  catalogue: Catalogue;
  onCreateCookbook: (cookbook: Cookbook) => Promise<{ ok: boolean; error?: string }>;
  onImportComplete?: () => Promise<void>;
  onUseImportDraft?: (recipeImport: RecipeImport) => void;
}

export function CookbookOverview({
  catalogue,
  onCreateCookbook,
  onImportComplete,
  onUseImportDraft,
}: CookbookOverviewProps) {
  const [selectedCookbookId, setSelectedCookbookId] = useState<string | null>(null);
  const [showCreateForm, setShowCreateForm] = useState(false);
  // While an import job runs, refresh the catalogue when its counters move
  // (throttled) so page texts and extracted recipes fill in live.
  const lastLiveRefreshRef = useRef({ at: 0, signature: "" });
  const handleLiveProgress = useCallback(
    (progress: CookbookImportProgress) => {
      const signature = [
        progress.processedCount,
        progress.sectionCount,
        progress.contentBlockCount,
        progress.recipeCount,
      ].join(":");
      const now = Date.now();
      const last = lastLiveRefreshRef.current;
      if (signature === last.signature || now - last.at < 4000) {
        return;
      }
      lastLiveRefreshRef.current = { at: now, signature };
      void onImportComplete?.();
    },
    [onImportComplete],
  );
  const authorsById = new Map(catalogue.authors.map((author) => [author.id, author] as const));
  const usersById = new Map(catalogue.users.map((user) => [user.id, user] as const));
  const currentUser = catalogue.users.find((user) => user.id === catalogue.currentUserId) ?? null;
  const currentFamilyId = currentUser?.familyId ?? catalogue.families[0]?.id ?? null;
  const selectedCookbook =
    catalogue.cookbooks.find((book) => book.id === selectedCookbookId) ?? null;

  if (selectedCookbook) {
    const recipes = catalogue.recipes.filter((recipe) => recipe.cookbookId === selectedCookbook.id);
    const sourceCounts = cookbookSourceCounts(catalogue, selectedCookbook.id);
    const sourceImports = cookbookSourceImports(catalogue, selectedCookbook.id);
    const sourceSections = cookbookSections(catalogue, selectedCookbook.id);
    const sourceBlocks = cookbookContentBlocks(catalogue, selectedCookbook.id);
    const sourcePages = cookbookPages(catalogue, selectedCookbook.id);

    return (
      <div className="space-y-4">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setSelectedCookbookId(null)}
        >
          Back to all cookbooks
        </Button>
        <Card>
          <CardHeader>
            <CardTitle>{selectedCookbook.title}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm text-muted-foreground">
            <p>
              {authorNames(selectedCookbook, authorsById)}
              {selectedCookbook.publisher ? ` · ${selectedCookbook.publisher}` : ""}
              {selectedCookbook.publishedYear ? ` · ${selectedCookbook.publishedYear}` : ""}
            </p>
            <div className="flex flex-wrap gap-2">
              <Badge variant="outline">
                Owner: {userName(selectedCookbook.ownerUserId, usersById)}
              </Badge>
              <Badge variant="outline">Shared: {shareLabel(selectedCookbook, usersById)}</Badge>
              <Badge variant="outline">{sourceCounts.imports} imports</Badge>
              <Badge variant="outline">{sourceCounts.pages} pages</Badge>
              <Badge variant="outline">{recipes.length} recipe cards</Badge>
              <Badge variant="outline">{sourceCounts.blocks} blocks</Badge>
            </div>
          </CardContent>
        </Card>
        <CookbookSourceImportPanel
          cookbook={selectedCookbook}
          counts={sourceCounts}
          existingImportPageCounts={sourceImports.map((source) => source.pages)}
          onImportComplete={onImportComplete}
        />
        <PipelineDiagnosticPanel cookbook={selectedCookbook} pageCount={sourceCounts.pages} />
        <CookbookSourceImportList
          imports={sourceImports}
          onProcessComplete={onImportComplete}
          onProgress={handleLiveProgress}
        />
        <CookbookSourceReview
          cookbook={selectedCookbook}
          pages={sourcePages}
          recipesById={new Map(recipes.map((recipe) => [recipe.id, recipe] as const))}
          menus={catalogue.cookbookMenus.filter((item) => item.cookbookId === selectedCookbook.id)}
          glossaryEntries={catalogue.cookbookGlossaryEntries.filter(
            (item) => item.cookbookId === selectedCookbook.id,
          )}
          suppliers={catalogue.cookbookSuppliers.filter(
            (item) => item.cookbookId === selectedCookbook.id,
          )}
          indexEntries={catalogue.cookbookIndexEntries.filter(
            (item) => item.cookbookId === selectedCookbook.id,
          )}
          crossReferences={catalogue.cookbookCrossReferences.filter(
            (item) => item.cookbookId === selectedCookbook.id,
          )}
          onPageUpdated={onImportComplete}
          onUseDraft={onUseImportDraft}
        />
        <CookbookDocumentView
          cookbook={selectedCookbook}
          sections={sourceSections}
          previewBlocks={sourceBlocks}
          recipes={recipes}
        />
        {recipes.length === 0 ? (
          <p className="text-sm text-muted-foreground">No recipe cards extracted yet.</p>
        ) : null}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <Button
          type="button"
          variant={showCreateForm ? "ghost" : "outline"}
          size="sm"
          onClick={() => setShowCreateForm((value) => !value)}
        >
          {showCreateForm ? "Cancel" : "New cookbook"}
        </Button>
      </div>

      {showCreateForm ? (
        <CookbookCreateForm
          authors={catalogue.authors}
          users={catalogue.users}
          currentUserId={catalogue.currentUserId}
          currentFamilyId={currentFamilyId}
          onCreate={async (cookbook) => {
            const result = await onCreateCookbook(cookbook);
            if (result.ok) {
              setShowCreateForm(false);
              setSelectedCookbookId(cookbook.id);
            }
            return result;
          }}
        />
      ) : null}

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {catalogue.cookbooks.map((cookbook) => {
          const recipeCount = catalogue.recipes.filter(
            (recipe) => recipe.cookbookId === cookbook.id,
          ).length;

          return (
            <button
              key={cookbook.id}
              type="button"
              className="overflow-hidden rounded-lg border bg-card text-left shadow-sm transition-colors hover:bg-accent/50"
              onClick={() => setSelectedCookbookId(cookbook.id)}
            >
              {cookbook.coverImageUrl ? (
                <img
                  src={cookbook.coverImageUrl}
                  alt={cookbook.title}
                  className="h-40 w-full object-cover"
                />
              ) : null}
              <div className="p-4">
                <p className="font-medium">{cookbook.title}</p>
                <p className="mt-1 text-sm text-muted-foreground">
                  {authorNames(cookbook, authorsById)}
                </p>
                <Badge variant="outline" className="mt-2">
                  {recipeCount} {recipeCount === 1 ? "recipe" : "recipes"}
                </Badge>
                <div className="mt-2 flex flex-wrap gap-1">
                  <Badge variant="outline">
                    Owner: {userName(cookbook.ownerUserId, usersById)}
                  </Badge>
                  <Badge variant="outline">Shared: {shareLabel(cookbook, usersById)}</Badge>
                </div>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function cookbookSourceCounts(catalogue: Catalogue, cookbookId: string) {
  return {
    imports: catalogue.cookbookImports.filter((item) => item.cookbookId === cookbookId).length,
    pages: catalogue.cookbookPages.filter((item) => item.cookbookId === cookbookId).length,
    sections: catalogue.cookbookSections.filter((item) => item.cookbookId === cookbookId).length,
    blocks: catalogue.cookbookContentBlocks.filter((item) => item.cookbookId === cookbookId).length,
    menus: catalogue.cookbookMenus.filter((item) => item.cookbookId === cookbookId).length,
    glossary: catalogue.cookbookGlossaryEntries.filter((item) => item.cookbookId === cookbookId)
      .length,
    suppliers: catalogue.cookbookSuppliers.filter((item) => item.cookbookId === cookbookId).length,
    index: catalogue.cookbookIndexEntries.filter((item) => item.cookbookId === cookbookId).length,
    references: catalogue.cookbookCrossReferences.filter((item) => item.cookbookId === cookbookId)
      .length,
  };
}

function cookbookSourceImports(catalogue: Catalogue, cookbookId: string) {
  return catalogue.cookbookImports
    .filter((item) => item.cookbookId === cookbookId)
    .map((item) => ({
      importRecord: item,
      pages: catalogue.cookbookPages.filter((page) => page.importId === item.id).length,
    }))
    .sort((left, right) => right.importRecord.createdAt.localeCompare(left.importRecord.createdAt));
}

function cookbookSections(catalogue: Catalogue, cookbookId: string) {
  return catalogue.cookbookSections
    .filter((item) => item.cookbookId === cookbookId)
    .sort((left, right) => left.position - right.position || left.title.localeCompare(right.title));
}

function cookbookContentBlocks(catalogue: Catalogue, cookbookId: string) {
  return catalogue.cookbookContentBlocks
    .filter((item) => item.cookbookId === cookbookId)
    .sort(
      (left, right) =>
        left.position - right.position || (left.pageStart ?? 0) - (right.pageStart ?? 0),
    );
}

function cookbookPages(catalogue: Catalogue, cookbookId: string) {
  const pages = catalogue.cookbookPages
    .filter((item) => item.cookbookId === cookbookId)
    .sort((left, right) => pageNumber(left) - pageNumber(right) || left.id.localeCompare(right.id));
  const seen = new Set<string>();
  return pages.filter((page) => {
    const key = `${page.printedPageNumber ?? "image"}:${page.printedPageLabel ?? page.imageIndex}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function pageNumber(page: CookbookPage) {
  return page.printedPageNumber ?? page.imageIndex;
}

function pipelineStageLabel(stage: CookbookImportProgress["stage"]) {
  return stage.replaceAll("_", " ");
}

function progressPercent(progress: CookbookImportProgress) {
  if (progress.state === "complete") {
    return 100;
  }
  if (progress.state === "failed" || progress.state === "canceled") {
    return 100;
  }
  if (progress.total != null && progress.total > 0 && progress.current != null) {
    return Math.max(0, Math.min(100, Math.round((progress.current / progress.total) * 100)));
  }
  return null;
}

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function CookbookSourceImportList({
  imports,
  onProcessComplete,
  onProgress,
}: {
  imports: Array<{
    importRecord: CookbookImport;
    pages: number;
  }>;
  onProcessComplete?: () => Promise<void>;
  onProgress?: (progress: CookbookImportProgress) => void;
}) {
  const [processingId, setProcessingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progressByImportId, setProgressByImportId] = useState<
    Record<string, CookbookImportProgress>
  >({});

  const observeImportProgress = useCallback(
    async (
      importRecord: CookbookImport,
      initialProgress: CookbookImportProgress,
      announceCompletion: boolean,
      isCancelled: () => boolean,
      signal?: AbortSignal,
    ) => {
      const isRegenerate = importRecord.status !== "uploaded";
      let progress = initialProgress;
      setProgressByImportId((current) => ({ ...current, [importRecord.id]: progress }));

      while (progress.state === "running" && !isCancelled()) {
        await sleep(1500);
        const progressResult = await getCookbookImportProgress(importRecord.id, signal);
        if (isCancelled()) {
          return;
        }
        if (!progressResult.ok) {
          setProcessingId(null);
          setError(progressResult.error);
          return;
        }
        progress = progressResult.progress;
        setProgressByImportId((current) => ({ ...current, [importRecord.id]: progress }));
        onProgress?.(progress);
      }

      setProcessingId(null);

      if (progress.state === "failed") {
        setError(progress.error ?? progress.message);
        return;
      }
      if (progress.state === "canceled") {
        setMessage("Cookbook import processing canceled.");
        return;
      }

      if (!announceCompletion) {
        return;
      }

      const prefix = isRegenerate ? "Regenerated" : "OCR processed";
      setMessage(
        `${prefix} ${progress.processedCount} pages, skipped ${progress.skippedCount}, failed ${progress.failedCount}. Mapped ${progress.sectionCount} sections, ${progress.contentBlockCount} context blocks, and ${progress.recipeCount} recipes.`,
      );
      await onProcessComplete?.();
    },
    [onProcessComplete, onProgress],
  );

  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();

    for (const { importRecord } of imports) {
      void (async () => {
        const result = await getCookbookImportProgress(importRecord.id, controller.signal);
        if (cancelled || !result.ok) {
          return;
        }
        setProgressByImportId((current) => ({
          ...current,
          [importRecord.id]: result.progress,
        }));
        if (result.progress.state === "running") {
          await observeImportProgress(
            importRecord,
            result.progress,
            false,
            () => cancelled,
            controller.signal,
          );
        }
      })();
    }

    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [imports, observeImportProgress]);

  if (imports.length === 0) {
    return null;
  }

  async function handleProcessOcr(importRecord: CookbookImport) {
    setProcessingId(importRecord.id);
    setMessage(null);
    setError(null);

    const result = await processCookbookImportOcr(importRecord.id, {
      refreshOcr: importRecord.status !== "uploaded",
    });

    if (!result.ok) {
      setProcessingId(null);
      setError(result.error);
      return;
    }

    await observeImportProgress(importRecord, result.progress, true, () => false);
  }

  async function handleCancelProcess(importRecord: CookbookImport) {
    setError(null);
    const result = await cancelCookbookImportProcessing(importRecord.id);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setProcessingId(null);
    setProgressByImportId((current) => ({
      ...current,
      [importRecord.id]: result.progress,
    }));
    setMessage("Cookbook import processing canceled.");
  }

  return (
    <div className="space-y-2">
      <p className="text-sm font-medium">Source imports</p>
      {message ? <p className="text-sm text-muted-foreground">{message}</p> : null}
      {error ? <p className="text-sm text-destructive">{error}</p> : null}
      <div className="grid gap-2 md:grid-cols-2">
        {imports.map(({ importRecord, pages }) => {
          const progress = progressByImportId[importRecord.id];
          const isRunning = processingId === importRecord.id || progress?.state === "running";

          return (
            <div key={importRecord.id} className="rounded-lg border bg-card p-3 text-sm">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="truncate font-medium">{importRecord.sourcePath}</p>
                  <p className="mt-1 text-xs text-muted-foreground">{importRecord.id}</p>
                </div>
                <Badge variant="outline">{importRecord.status.replace("_", " ")}</Badge>
              </div>
              <div className="mt-3 flex flex-wrap gap-1">
                <Badge variant="outline">{pages} pages</Badge>
                {importRecord.ocrEngine ? (
                  <Badge variant="outline">{importRecord.ocrEngine}</Badge>
                ) : null}
              </div>
              <div className="mt-3 flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={isRunning}
                  onClick={() => void handleProcessOcr(importRecord)}
                >
                  {importRecord.status === "uploaded" ? (
                    <FileText
                      className={isRunning ? "h-4 w-4 animate-pulse" : "h-4 w-4"}
                      aria-hidden="true"
                    />
                  ) : (
                    <RefreshCw
                      className={isRunning ? "h-4 w-4 animate-spin" : "h-4 w-4"}
                      aria-hidden="true"
                    />
                  )}
                  {isRunning
                    ? importRecord.status === "uploaded"
                      ? "Processing OCR"
                      : "Regenerating"
                    : importRecord.status === "uploaded"
                      ? "Process OCR"
                      : "Regenerate"}
                </Button>
                {isRunning ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="border-destructive text-destructive hover:bg-destructive hover:text-destructive-foreground"
                    onClick={() => void handleCancelProcess(importRecord)}
                  >
                    <CircleStop className="h-4 w-4" aria-hidden="true" />
                    Cancel
                  </Button>
                ) : null}
              </div>
              {progress ? <CookbookImportProgressView progress={progress} /> : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CookbookImportProgressView({ progress }: { progress: CookbookImportProgress }) {
  const percent = progressPercent(progress);
  const countSummary = [
    `${progress.processedCount} processed`,
    `${progress.skippedCount} skipped`,
    `${progress.failedCount} failed`,
  ].join(" · ");
  const extractionSummary = [
    `${progress.sectionCount} sections`,
    `${progress.contentBlockCount} text blocks`,
    `${progress.recipeCount} recipes`,
  ].join(" · ");

  return (
    <div className="mt-3 space-y-2 rounded-md border bg-muted/30 p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm font-medium capitalize">{pipelineStageLabel(progress.stage)}</p>
        <Badge
          variant="outline"
          className={
            progress.state === "failed" || progress.state === "canceled"
              ? "border-destructive text-destructive"
              : ""
          }
        >
          {progress.state}
        </Badge>
      </div>
      <p className="text-xs text-muted-foreground">{progress.message}</p>
      <div
        className="h-2 overflow-hidden rounded-full bg-background"
        role="progressbar"
        aria-label="Cookbook import progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className="h-full bg-primary transition-all" style={{ width: `${percent ?? 12}%` }} />
      </div>
      <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
        {percent != null ? <span>{percent}%</span> : null}
        <span>{countSummary}</span>
        <span>{extractionSummary}</span>
        {progress.extractionEngine ? <span>{progress.extractionEngine}</span> : null}
      </div>
      {progress.currentSectionTitle ? (
        <p className="text-xs text-muted-foreground">
          Section {progress.currentSectionIndex ?? 0} of {progress.sectionTotal ?? "?"}:{" "}
          {progress.currentSectionTitle}
        </p>
      ) : null}
      {progress.error ? <p className="text-xs text-destructive">{progress.error}</p> : null}
    </div>
  );
}

function PipelineDiagnosticPanel({
  cookbook,
  pageCount,
}: {
  cookbook: Cookbook;
  pageCount: number;
}) {
  const [progress, setProgress] = useState<CookbookImportProgress | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [isIntroRunning, setIsIntroRunning] = useState(false);
  const [introDiagnostic, setIntroDiagnostic] = useState<IntroductionPageDiagnostic | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canRunIntroDiagnostic = cookbook.id === "our-korean-kitchen";

  const observeDiagnosticProgress = useCallback(
    async (initialProgress: CookbookImportProgress, isCancelled: () => boolean) => {
      let nextProgress = initialProgress;
      setProgress(nextProgress);

      while (nextProgress.state === "running" && !isCancelled()) {
        await sleep(1500);
        const result = await getPipelineDiagnosticProgress(nextProgress.importId);
        if (isCancelled()) {
          return;
        }
        if (!result.ok) {
          setIsRunning(false);
          setError(result.error);
          return;
        }
        nextProgress = result.progress;
        setProgress(nextProgress);
      }

      setIsRunning(false);

      if (nextProgress.state === "failed") {
        setError(nextProgress.error ?? nextProgress.message);
        return;
      }
      if (nextProgress.state === "canceled") {
        setMessage("Pipeline check canceled.");
        return;
      }

      const processedPageLabel =
        nextProgress.processedCount === 1
          ? `${nextProgress.processedCount} page`
          : `${nextProgress.processedCount} pages`;
      setMessage(
        `Pipeline check complete. OCR ${processedPageLabel}, ${nextProgress.sectionCount} sections, ${nextProgress.contentBlockCount} text blocks, ${nextProgress.recipeCount} recipes.`,
      );
    },
    [],
  );

  async function handleStartDiagnostic() {
    setIsRunning(true);
    setIntroDiagnostic(null);
    setMessage(null);
    setError(null);

    const result = await startCookbookPipelineDiagnostic(cookbook.id);
    if (!result.ok) {
      setIsRunning(false);
      setError(result.error);
      return;
    }

    await observeDiagnosticProgress(result.progress, () => false);
  }

  async function handleStartIntroDiagnostic() {
    setIsIntroRunning(true);
    setIntroDiagnostic(null);
    setMessage(null);
    setError(null);

    const result = await runIntroductionPageDiagnostic(cookbook.id);
    setIsIntroRunning(false);
    if (!result.ok) {
      setIsIntroRunning(false);
      setError(result.error);
      return;
    }

    let nextProgress = result.progress;
    setProgress(nextProgress);

    while (nextProgress.state === "running") {
      await sleep(1500);
      const progressResult = await getPipelineDiagnosticProgress(nextProgress.importId);
      if (!progressResult.ok) {
        setIsIntroRunning(false);
        setError(progressResult.error);
        return;
      }
      nextProgress = progressResult.progress;
      setProgress(nextProgress);
    }

    setIsIntroRunning(false);
    if (nextProgress.state === "failed") {
      setError(nextProgress.error ?? nextProgress.message);
      return;
    }
    if (nextProgress.state === "canceled") {
      setMessage("Introduction page check canceled.");
      return;
    }

    const diagnosticResult = await getIntroductionPageDiagnostic(nextProgress.importId);
    if (!diagnosticResult.ok) {
      setError(diagnosticResult.error);
      return;
    }

    setIntroDiagnostic(diagnosticResult.diagnostic);
    setMessage(
      diagnosticResult.diagnostic.checksPassed
        ? "Introduction page check passed."
        : `Introduction page check found ${diagnosticResult.diagnostic.issues.length} issue${diagnosticResult.diagnostic.issues.length === 1 ? "" : "s"}.`,
    );
  }

  async function handleCancelDiagnostic() {
    if (progress?.state !== "running") {
      return;
    }
    setError(null);
    const result = await cancelPipelineDiagnostic(progress.importId);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setIsRunning(false);
    setProgress(result.progress);
    setMessage("Pipeline check canceled.");
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Pipeline check</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap gap-2">
            <Badge variant="outline">{pageCount} source pages</Badge>
            {progress?.extractionEngine ? (
              <Badge variant="outline">{progress.extractionEngine}</Badge>
            ) : null}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isRunning || isIntroRunning || pageCount === 0}
              onClick={() => void handleStartDiagnostic()}
            >
              <Activity
                className={isRunning ? "h-4 w-4 animate-pulse" : "h-4 w-4"}
                aria-hidden="true"
              />
              {isRunning ? "Checking pipeline" : "Run pipeline check"}
            </Button>
            {isRunning ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="border-destructive text-destructive hover:bg-destructive hover:text-destructive-foreground"
                onClick={handleCancelDiagnostic}
              >
                <CircleStop className="h-4 w-4" aria-hidden="true" />
                Cancel
              </Button>
            ) : null}
            {canRunIntroDiagnostic ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={isIntroRunning || isRunning || pageCount === 0}
                onClick={() => void handleStartIntroDiagnostic()}
              >
                <FileText
                  className={isIntroRunning ? "h-4 w-4 animate-pulse" : "h-4 w-4"}
                  aria-hidden="true"
                />
                {isIntroRunning ? "Checking intro page" : "Run intro page check"}
              </Button>
            ) : null}
          </div>
        </div>
        {message ? <p className="text-sm text-muted-foreground">{message}</p> : null}
        {error ? <p className="text-sm text-destructive">{error}</p> : null}
        {progress ? <CookbookImportProgressView progress={progress} /> : null}
        {introDiagnostic ? <IntroductionPageDiagnosticView diagnostic={introDiagnostic} /> : null}
      </CardContent>
    </Card>
  );
}

function IntroductionPageDiagnosticView({
  diagnostic,
}: {
  diagnostic: IntroductionPageDiagnostic;
}) {
  const artifactRows = [
    ["OCR text", diagnostic.artifacts.ocrTextPath],
    ["OCR output", diagnostic.artifacts.ocrOutputPath],
    ["Source-map input", diagnostic.artifacts.sourceMapInputPath],
    ["Source-map output", diagnostic.artifacts.sourceMapOutputPath],
    ["DeepSeek input", diagnostic.artifacts.deepseekInputPath],
    ["DeepSeek output", diagnostic.artifacts.deepseekOutputPath],
    ["DeepSeek verbose", diagnostic.artifacts.deepseekVerboseDir],
  ] as const;

  return (
    <div className="space-y-3 rounded-md border bg-muted/30 p-3 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <Badge
          variant={diagnostic.checksPassed ? "default" : "outline"}
          className={diagnostic.checksPassed ? "" : "border-destructive text-destructive"}
        >
          {diagnostic.checksPassed ? "Passed" : "Needs review"}
        </Badge>
        <Badge variant="outline">image {diagnostic.imageIndex}</Badge>
        <Badge variant="outline">selected by {diagnostic.selectedBy}</Badge>
        <Badge variant="outline">{diagnostic.ocrEngine}</Badge>
        <Badge variant="outline">{diagnostic.extractionEngine}</Badge>
      </div>
      <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
        <span>
          printed page{" "}
          {diagnostic.detectedPrintedPageNumber ?? diagnostic.storedPrintedPageNumber ?? "unknown"}
        </span>
        <span>{diagnostic.ocrLayoutMode ?? "layout unknown"}</span>
        <span>{diagnostic.ocrColumnDetection ?? "columns unknown"}</span>
        <span>{diagnostic.sourceMapContentBlockCount} source blocks</span>
        <span>{diagnostic.extractedContentBlockCount} extracted blocks</span>
      </div>
      {diagnostic.issues.length > 0 ? (
        <div className="space-y-1">
          <p className="text-xs font-medium text-destructive">Issues</p>
          <ul className="space-y-1 text-xs text-destructive">
            {diagnostic.issues.map((issue) => (
              <li key={issue}>{issue}</li>
            ))}
          </ul>
        </div>
      ) : null}
      <div className="space-y-1">
        <p className="text-xs font-medium">Artifacts</p>
        <dl className="grid gap-1 text-xs text-muted-foreground md:grid-cols-[9rem_1fr]">
          {artifactRows.map(([label, path]) => (
            <div key={label} className="contents">
              <dt>{label}</dt>
              <dd className="break-all font-mono">{path}</dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}

function CookbookSourceImportPanel({
  cookbook,
  counts,
  existingImportPageCounts,
  onImportComplete,
}: {
  cookbook: Cookbook;
  counts: ReturnType<typeof cookbookSourceCounts>;
  existingImportPageCounts: number[];
  onImportComplete?: () => Promise<void>;
}) {
  const [sourcePath, setSourcePath] = useState(`imports/${cookbook.id}`);
  const [files, setFiles] = useState<File[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [uploadProgress, setUploadProgress] = useState<{
    loaded: number;
    total: number | null;
    percent: number | null;
  } | null>(null);
  const [allowAdditionalImport, setAllowAdditionalImport] = useState(false);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    setMessage(null);
    setError(null);
    setUploadProgress(null);

    if (files.length === 0) {
      setError("Choose at least one image.");
      return;
    }

    const normalizedSourcePath = sourcePath.trim().replace(/\/+$/g, "");
    if (!normalizedSourcePath) {
      setError("Source path is required.");
      return;
    }
    const matchedExistingImportPageCount = existingImportPageCounts.find(
      (pageCount) => pageCount > 0 && pageCount === files.length,
    );
    if (
      !allowAdditionalImport &&
      counts.pages > 0 &&
      (matchedExistingImportPageCount !== undefined || files.length >= counts.pages)
    ) {
      setAllowAdditionalImport(true);
      const existingPageCount = matchedExistingImportPageCount ?? counts.pages;
      setError(
        `This cookbook already has a ${existingPageCount}-page source import. Press Import again to add another source import.`,
      );
      return;
    }

    setIsSubmitting(true);
    let archive: Blob;
    try {
      setMessage(`Packaging ${files.length} images into one archive.`);
      archive = buildTarArchive(files);
    } catch (err) {
      setIsSubmitting(false);
      setError(err instanceof Error ? err.message : "Could not package the selected images.");
      return;
    }

    setMessage(
      `Uploading one ${(archive.size / 1024 / 1024).toFixed(1)} MB archive for ${files.length} images.`,
    );
    setUploadProgress({ loaded: 0, total: archive.size, percent: 0 });
    const result = await createCookbookArchiveImport({
      cookbookId: cookbook.id,
      sourcePath: normalizedSourcePath,
      archive,
      onUploadProgress: (progress) => {
        setUploadProgress(progress);
        if (progress.percent === 100) {
          setMessage("Upload complete. the server is unpacking and hashing the archive.");
        }
      },
    });
    setIsSubmitting(false);

    if (!result.ok) {
      setError(result.error);
      return;
    }

    setMessage(
      `Imported ${result.summary.pageCount} pages as ${result.summary.importRecord.status}.`,
    );
    setUploadProgress(null);
    setFiles([]);
    setAllowAdditionalImport(false);
    await onImportComplete?.();
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Cookbook import</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <form className="grid gap-3 md:grid-cols-[1fr_1fr_auto]" onSubmit={handleSubmit}>
          <label className="space-y-1 text-sm" htmlFor="cookbook-source-path">
            <span>Source path</span>
            <Input
              id="cookbook-source-path"
              value={sourcePath}
              onChange={(event) => {
                setSourcePath(event.target.value);
                setAllowAdditionalImport(false);
              }}
              required
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-source-images">
            <span>Images</span>
            <Input
              id="cookbook-source-images"
              type="file"
              accept="image/*"
              multiple
              onChange={(event) => {
                const selected = Array.from(event.target.files ?? []).sort((left, right) =>
                  left.name.localeCompare(right.name),
                );
                setFiles(selected);
                setAllowAdditionalImport(false);
                setUploadProgress(null);
              }}
            />
          </label>
          <div className="flex items-end">
            <Button type="submit" disabled={isSubmitting || files.length === 0}>
              Import
            </Button>
          </div>
          {message ? (
            <p className="text-sm text-muted-foreground md:col-span-3">{message}</p>
          ) : null}
          {uploadProgress ? (
            <div className="space-y-1 md:col-span-3">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>Upload progress</span>
                <span>
                  {uploadProgress.percent !== null ? `${uploadProgress.percent}%` : "Uploading"}
                </span>
              </div>
              <div
                aria-label="Upload progress"
                aria-valuemax={100}
                aria-valuemin={0}
                aria-valuenow={uploadProgress.percent ?? undefined}
                className="h-2 w-full overflow-hidden rounded-full bg-muted"
                role="progressbar"
              >
                <div
                  className="h-full bg-primary transition-[width] duration-200"
                  style={{ width: `${uploadProgress.percent ?? 0}%` }}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {formatUploadBytes(uploadProgress.loaded)}
                {uploadProgress.total ? ` of ${formatUploadBytes(uploadProgress.total)}` : ""} sent
              </p>
            </div>
          ) : null}
          {error ? <p className="text-sm text-destructive md:col-span-3">{error}</p> : null}
        </form>
        <div className="flex flex-wrap gap-2">
          <Badge variant="outline">{counts.sections} sections</Badge>
          <Badge variant="outline">{counts.menus} menus</Badge>
          <Badge variant="outline">{counts.glossary} glossary</Badge>
          <Badge variant="outline">{counts.suppliers} suppliers</Badge>
          <Badge variant="outline">{counts.index} index</Badge>
          <Badge variant="outline">{counts.references} references</Badge>
        </div>
      </CardContent>
    </Card>
  );
}

function CookbookCreateForm({
  authors,
  users,
  currentUserId,
  currentFamilyId,
  onCreate,
}: {
  authors: Author[];
  users: User[];
  currentUserId: string | null;
  currentFamilyId: string | null;
  onCreate: (cookbook: Cookbook) => Promise<{ ok: boolean; error?: string }>;
}) {
  const [title, setTitle] = useState("");
  const [id, setId] = useState("");
  const [idTouched, setIdTouched] = useState(false);
  const [selectedAuthorIds, setSelectedAuthorIds] = useState<string[]>(
    authors[0] ? [authors[0].id] : [],
  );
  const [isbn, setIsbn] = useState("");
  const [publisher, setPublisher] = useState("");
  const [publishedYear, setPublishedYear] = useState("");
  const [coverImageUrl, setCoverImageUrl] = useState("");
  const [shareScope, setShareScope] = useState<ShareScope>("family");
  const [sharedWithUserIds, setSharedWithUserIds] = useState<string[]>([]);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const shareableUsers = users.filter((user) => user.id !== currentUserId);

  function handleTitleChange(value: string) {
    setTitle(value);
    if (!idTouched) {
      setId(slugify(value));
    }
  }

  function toggleAuthor(authorId: string) {
    setSelectedAuthorIds((current) =>
      current.includes(authorId)
        ? current.filter((value) => value !== authorId)
        : [...current, authorId],
    );
  }

  function toggleSharedUser(userId: string) {
    setSharedWithUserIds((current) =>
      current.includes(userId) ? current.filter((value) => value !== userId) : [...current, userId],
    );
  }

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    setError(null);

    if (!title.trim() || !id.trim()) {
      setError("Title and cookbook ID are required.");
      return;
    }
    if (!/^[a-z0-9][a-z0-9-]{1,79}$/.test(id.trim())) {
      setError("Cookbook ID must be lowercase letters, numbers, and hyphens.");
      return;
    }
    if (selectedAuthorIds.length === 0) {
      setError("Choose at least one author.");
      return;
    }
    if (shareScope === "users" && sharedWithUserIds.length === 0) {
      setError("Choose at least one user to share with.");
      return;
    }

    setIsSubmitting(true);
    const result = await onCreate({
      id: id.trim(),
      title: title.trim(),
      authorIds: selectedAuthorIds,
      isbn: isbn.trim() || null,
      publisher: publisher.trim() || null,
      publishedYear: toYearOrNull(publishedYear),
      coverImageUrl: coverImageUrl.trim() || null,
      ownerUserId: currentUserId,
      familyId: currentFamilyId,
      shareScope,
      sharedWithUserIds: shareScope === "users" ? sharedWithUserIds : [],
    });
    setIsSubmitting(false);

    if (!result.ok) {
      setError(result.error ?? "Could not create cookbook.");
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>New cookbook</CardTitle>
      </CardHeader>
      <CardContent>
        <form className="grid gap-3 sm:grid-cols-2" onSubmit={handleSubmit}>
          <label className="space-y-1 text-sm" htmlFor="cookbook-title">
            <span>Title</span>
            <Input
              id="cookbook-title"
              value={title}
              onChange={(event) => handleTitleChange(event.target.value)}
              required
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-id">
            <span>Cookbook ID</span>
            <Input
              id="cookbook-id"
              value={id}
              onChange={(event) => {
                setIdTouched(true);
                setId(event.target.value);
              }}
              required
            />
          </label>
          <div className="space-y-1 text-sm sm:col-span-2">
            <span>Authors</span>
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
          </div>
          <label className="space-y-1 text-sm" htmlFor="cookbook-isbn">
            <span>ISBN</span>
            <Input
              id="cookbook-isbn"
              value={isbn}
              onChange={(event) => setIsbn(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-publisher">
            <span>Publisher</span>
            <Input
              id="cookbook-publisher"
              value={publisher}
              onChange={(event) => setPublisher(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-year">
            <span>Published year</span>
            <Input
              id="cookbook-year"
              type="number"
              min="1400"
              max="2600"
              value={publishedYear}
              onChange={(event) => setPublishedYear(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-cover">
            <span>Cover image URL</span>
            <Input
              id="cookbook-cover"
              type="url"
              value={coverImageUrl}
              onChange={(event) => setCoverImageUrl(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="cookbook-sharing">
            <span>Sharing</span>
            <select
              id="cookbook-sharing"
              className={selectClassName}
              value={shareScope}
              onChange={(event) => setShareScope(event.target.value as ShareScope)}
            >
              <option value="personal">Personal</option>
              <option value="family">Family</option>
              <option value="users">Selected users</option>
            </select>
          </label>
          {shareScope === "users" ? (
            <div className="space-y-1 text-sm sm:col-span-2">
              <span>Shared users</span>
              <div className="flex flex-wrap gap-2">
                {shareableUsers.map((user) => (
                  <Button
                    key={user.id}
                    type="button"
                    size="sm"
                    variant={sharedWithUserIds.includes(user.id) ? "default" : "outline"}
                    onClick={() => toggleSharedUser(user.id)}
                  >
                    {user.displayName}
                  </Button>
                ))}
              </div>
            </div>
          ) : null}
          {error ? <p className="text-sm text-destructive sm:col-span-2">{error}</p> : null}
          <div className="flex gap-2 sm:col-span-2">
            <Button type="submit" disabled={isSubmitting}>
              Create cookbook
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
