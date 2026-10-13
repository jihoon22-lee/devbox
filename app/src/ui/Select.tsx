import * as S from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import s from "./Select.module.css";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}
export function Select({
  label,
  options,
  placeholder = "선택",
  className,
  onChange,
  onValueChange,
  ...props
}: S.SelectProps & {
  label: string;
  options: SelectOption[];
  placeholder?: string;
  className?: string;
  onChange?: (value: string) => void;
}) {
  return (
    <S.Root {...props} onValueChange={onChange ?? onValueChange}>
      <S.Trigger aria-label={label} className={[s.trigger, className].filter(Boolean).join(" ")}>
        <S.Value placeholder={placeholder} />
        <S.Icon>
          <ChevronDown size={14} aria-hidden="true" />
        </S.Icon>
      </S.Trigger>
      <S.Portal>
        <S.Content className={s.content} position="popper" sideOffset={4}>
          <S.Viewport>
            {options.map((option) => (
              <S.Item key={option.value} value={option.value} disabled={option.disabled} className={s.item}>
                <S.ItemText>{option.label}</S.ItemText>
                <S.ItemIndicator>
                  <Check size={14} aria-hidden="true" />
                </S.ItemIndicator>
              </S.Item>
            ))}
          </S.Viewport>
        </S.Content>
      </S.Portal>
    </S.Root>
  );
}
