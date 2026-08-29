import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { IngredientOption } from "@/lib/ingredients";
import type { PantryCategory, PantryItem } from "@/lib/schema";
import { formatDate } from "@/lib/utils";

const CATEGORY_LABELS: Record<PantryCategory, string> = {
  raw: "Raw ingredients",
  prepared: "Prepared components",
  leftover: "Leftovers",
};

const CATEGORY_ORDER: PantryCategory[] = ["raw", "prepared", "leftover"];

const UNIT_OPTIONS = [
  "g",
  "kg",
  "ml",
  "l",
  "tsp",
  "tbsp",
  "cup",
  "oz",
  "lb",
  "each",
  "tin",
  "clove",
  "bunch",
  "serving",
];

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

export interface PantryItemFormInput {
  item: string;
  displayName: string;
  quantity: number | null;
  unit: string | null;
  category: PantryCategory;
  notes: string | null;
  expiresAt: string | null;
}

interface PantryViewProps {
  pantryItems: PantryItem[];
  ingredientOptions: IngredientOption[];
  familyName: string | null;
  isShared: boolean;
  onAdd: (input: PantryItemFormInput) => Promise<void>;
  onAdjustQuantity: (id: string, quantity: number) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
}

export function PantryView({
  pantryItems,
  ingredientOptions,
  familyName,
  isShared,
  onAdd,
  onAdjustQuantity,
  onDelete,
}: PantryViewProps) {
  const [item, setItem] = useState("");
  const [quantity, setQuantity] = useState("");
  const [unit, setUnit] = useState("");
  const [category, setCategory] = useState<PantryCategory>("raw");
  const [notes, setNotes] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);

  const grouped = useMemo(() => {
    const groups: Record<PantryCategory, PantryItem[]> = { raw: [], prepared: [], leftover: [] };
    for (const pantryItem of pantryItems) {
      groups[pantryItem.category].push(pantryItem);
    }
    return groups;
  }, [pantryItems]);

  async function handleSubmit(event: React.SubmitEvent) {
    event.preventDefault();
    const trimmedItem = item.trim();
    if (trimmedItem.length === 0) {
      return;
    }

    setIsSubmitting(true);
    await onAdd({
      item: trimmedItem,
      displayName: trimmedItem,
      quantity: quantity.trim().length > 0 ? Number(quantity) : null,
      unit: unit.trim().length > 0 ? unit.trim() : null,
      category,
      notes: notes.trim().length > 0 ? notes.trim() : null,
      expiresAt: expiresAt.trim().length > 0 ? expiresAt.trim() : null,
    });
    setIsSubmitting(false);
    setItem("");
    setQuantity("");
    setUnit("");
    setNotes("");
    setExpiresAt("");
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <CardTitle>{familyName ? `${familyName} pantry` : "Pantry"}</CardTitle>
            {isShared ? <Badge variant="outline">Family shared</Badge> : null}
          </div>
        </CardHeader>
        <CardContent>
          <form className="grid gap-3 sm:grid-cols-2 lg:grid-cols-6" onSubmit={handleSubmit}>
            <Input
              className="lg:col-span-2"
              list="pantry-ingredient-options"
              placeholder="Ingredient"
              value={item}
              onChange={(event) => setItem(event.target.value)}
              required
            />
            <datalist id="pantry-ingredient-options">
              {ingredientOptions.map((option) => (
                <option key={option.item} value={option.displayName} />
              ))}
            </datalist>
            <Input
              type="number"
              step="any"
              min="0"
              placeholder="Quantity"
              value={quantity}
              onChange={(event) => setQuantity(event.target.value)}
            />
            <select
              className={selectClassName}
              value={unit}
              onChange={(event) => setUnit(event.target.value)}
            >
              <option value="">Unit</option>
              {UNIT_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
            <select
              className={selectClassName}
              value={category}
              onChange={(event) => setCategory(event.target.value as PantryCategory)}
            >
              <option value="raw">Raw</option>
              <option value="prepared">Prepared</option>
              <option value="leftover">Leftover</option>
            </select>
            <Input
              type="date"
              value={expiresAt}
              onChange={(event) => setExpiresAt(event.target.value)}
              aria-label="Expiry date"
            />
            <Button type="submit" disabled={isSubmitting}>
              Add
            </Button>
            <Input
              className="sm:col-span-2 lg:col-span-6"
              placeholder="Notes (optional)"
              value={notes}
              onChange={(event) => setNotes(event.target.value)}
            />
          </form>
        </CardContent>
      </Card>

      {CATEGORY_ORDER.map((categoryKey) => (
        <Card key={categoryKey}>
          <CardHeader>
            <CardTitle>{CATEGORY_LABELS[categoryKey]}</CardTitle>
          </CardHeader>
          <CardContent>
            {grouped[categoryKey].length === 0 ? (
              <p className="text-sm text-muted-foreground">Nothing here yet.</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Item</TableHead>
                    <TableHead>Quantity</TableHead>
                    <TableHead>Expires</TableHead>
                    <TableHead>Notes</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {grouped[categoryKey].map((pantryItem) => (
                    <PantryRow
                      key={pantryItem.id}
                      pantryItem={pantryItem}
                      onAdjustQuantity={onAdjustQuantity}
                      onDelete={onDelete}
                    />
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function PantryRow({
  pantryItem,
  onAdjustQuantity,
  onDelete,
}: {
  pantryItem: PantryItem;
  onAdjustQuantity: (id: string, quantity: number) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
}) {
  const [quantity, setQuantity] = useState(pantryItem.quantity?.toString() ?? "");

  return (
    <TableRow>
      <TableCell>
        <div className="font-medium">{pantryItem.displayName}</div>
        {pantryItem.sourceRecipeId ? (
          <Badge variant="outline" className="mt-1">
            From a recipe
          </Badge>
        ) : null}
      </TableCell>
      <TableCell>
        <div className="flex items-center gap-2">
          <Input
            className="h-8 w-24"
            type="number"
            step="any"
            min="0"
            value={quantity}
            onChange={(event) => setQuantity(event.target.value)}
            onBlur={() => {
              const parsed = Number(quantity);
              if (!Number.isNaN(parsed) && parsed >= 0) {
                void onAdjustQuantity(pantryItem.id, parsed);
              }
            }}
          />
          <span className="text-sm text-muted-foreground">{pantryItem.unit ?? ""}</span>
        </div>
      </TableCell>
      <TableCell>{pantryItem.expiresAt ? formatDate(pantryItem.expiresAt) : "—"}</TableCell>
      <TableCell className="text-sm text-muted-foreground">{pantryItem.notes ?? "—"}</TableCell>
      <TableCell className="text-right">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void onDelete(pantryItem.id)}
        >
          Remove
        </Button>
      </TableCell>
    </TableRow>
  );
}
