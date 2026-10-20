import type { ReactNode } from "react";
import s from "./DataTable.module.css";
export function DataTable<T>({
  rows,
  columns,
  rowKey,
  label,
}: {
  rows: T[];
  columns: { key: string; title: string; render: (row: T) => ReactNode }[];
  rowKey: (row: T) => string;
  label: string;
}) {
  return (
    <table className={s.table} aria-label={label}>
      <thead>
        <tr>
          {columns.map((column) => (
            <th key={column.key} scope="col">
              {column.title}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={rowKey(row)}>
            {columns.map((column) => (
              <td key={column.key}>{column.render(row)}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
