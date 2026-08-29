import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { CookLogEntry, Recipe } from "@/lib/schema";
import { formatDate } from "@/lib/utils";

interface HistoryViewProps {
  cookLog: CookLogEntry[];
  recipesById: Map<string, Recipe>;
}

export function HistoryView({ cookLog, recipesById }: HistoryViewProps) {
  if (cookLog.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>No cook history yet</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          Mark a recipe as made to start building history here.
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="space-y-3">
      {cookLog.map((entry) => {
        const recipe = recipesById.get(entry.recipeId);

        return (
          <Card key={entry.id}>
            <CardHeader>
              <CardTitle className="flex flex-wrap items-center justify-between gap-2 text-base">
                <span>{recipe?.title ?? entry.recipeId}</span>
                <span className="text-sm font-normal text-muted-foreground">
                  {formatDate(entry.madeAt)}
                </span>
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-sm">
              <div className="flex flex-wrap gap-2">
                {entry.servingsMade != null ? (
                  <Badge variant="outline">Made {entry.servingsMade} servings</Badge>
                ) : null}
                {entry.servingsEaten != null ? (
                  <Badge variant="outline">Ate {entry.servingsEaten}</Badge>
                ) : null}
                {entry.leftoverServings != null ? (
                  <Badge variant="outline">{entry.leftoverServings} leftover</Badge>
                ) : null}
              </div>
              {entry.substitutions.length > 0 ? (
                <div>
                  <p className="text-xs font-medium uppercase text-muted-foreground">
                    Substitutions
                  </p>
                  <ul className="mt-1 space-y-1">
                    {entry.substitutions.map((substitution) => (
                      <li key={substitution.id || `${entry.id}-${substitution.ingredientId}`}>
                        {substitution.originalItem || substitution.ingredientId} →{" "}
                        {substitution.substituteText}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
              {entry.notes ? <p className="text-muted-foreground">{entry.notes}</p> : null}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}
