import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export interface FilterOption {
  value: string;
  label: string;
}

/**
 * A labelled filter dropdown for table filter bars. The sentinel option whose
 * value equals `allValue` (default "all") clears the filter (reported as `null`)
 * so callers can keep it out of the URL/query.
 */
export function FilterSelect({
  value,
  onChange,
  options,
  label,
  className = "w-40",
  allValue = "all",
}: {
  value: string;
  onChange: (value: string | null) => void;
  options: FilterOption[];
  label: string;
  className?: string;
  allValue?: string;
}) {
  return (
    <Select
      value={value}
      onValueChange={(val) => onChange(val === allValue ? null : val)}
    >
      <SelectTrigger className={className} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((opt) => (
          <SelectItem key={opt.value} value={opt.value}>
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
