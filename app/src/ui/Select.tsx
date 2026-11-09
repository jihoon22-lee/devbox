import * as S from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import { useState } from "react";
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
  id,
  onChange,
  onValueChange,
  ...props
}: S.SelectProps & {
  label: string;
  options: SelectOption[];
  placeholder?: string;
  className?: string;
  id?: string;
  onChange?: (value: string) => void;
}) {
  // The trigger shows one line, so its title names the whole chosen label, also when uncontrolled.
  const [picked, setPicked] = useState(props.defaultValue);
  const chosen = options.find((option) => option.value === (props.value ?? picked))?.label;
  return (
    <S.Root
      {...props}
      onValueChange={(value) => {
        setPicked(value);
        (onChange ?? onValueChange)?.(value);
      }}
    >
      <S.Trigger id={id} aria-label={label} title={chosen} className={[s.trigger, className].filter(Boolean).join(" ")}>
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
