import type { Rule } from "@/lib/types";
import { Button } from "@/components/ui/Button";
import { Plus } from "lucide-react";
import { RuleBuilder } from "./RuleBuilder";
import { createDefaultRule } from "./utils";

interface RulesListProps {
  title: string;
  rules: Rule[];
  onChange: (rules: Rule[]) => void;
}

export function RulesList({ title, rules, onChange }: RulesListProps) {
  const handleAdd = () => {
    const newRules = [...rules];
    // Set AND connector on the previous last rule
    if (newRules.length > 0) {
      newRules[newRules.length - 1] = {
        ...newRules[newRules.length - 1],
        logical_operator: newRules[newRules.length - 1].logical_operator ?? "AND",
      };
    }
    newRules.push(createDefaultRule());
    onChange(newRules);
  };

  const handleUpdate = (index: number, updated: Rule) => {
    const newRules = [...rules];
    newRules[index] = updated;
    onChange(newRules);
  };

  const handleDelete = (index: number) => {
    const newRules = rules.filter((_, i) => i !== index);
    // Clear logical_operator on the new last rule
    if (newRules.length > 0) {
      newRules[newRules.length - 1] = {
        ...newRules[newRules.length - 1],
        logical_operator: undefined,
      };
    }
    onChange(newRules);
  };

  return (
    <div className="space-y-3">
      <h3 className="text-sm font-semibold">{title}</h3>

      {rules.length === 0 && (
        <p className="text-xs text-muted-foreground">
          No rules defined. Add a rule to get started.
        </p>
      )}

      {rules.map((rule, index) => (
        <RuleBuilder
          key={rule.id}
          rule={rule}
          onChange={(updated) => handleUpdate(index, updated)}
          onDelete={() => handleDelete(index)}
          showLogicalOp={index < rules.length - 1}
        />
      ))}

      <Button variant="outline" size="sm" onClick={handleAdd}>
        <Plus className="mr-1.5 h-4 w-4" />
        Add Rule
      </Button>
    </div>
  );
}
