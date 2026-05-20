import { useMemo, useState } from "preact/hooks";
import type { ComponentChild } from "preact";

interface VirtualizedListProps<T> {
  items: T[];
  rowHeight: number;
  maxHeight: number;
  overscan?: number;
  className?: string;
  keyExtractor: (item: T, index: number) => string;
  renderItem: (item: T, index: number) => ComponentChild;
}

export function VirtualizedList<T>({
  items,
  rowHeight,
  maxHeight,
  overscan = 8,
  className,
  keyExtractor,
  renderItem,
}: VirtualizedListProps<T>) {
  const [scrollTop, setScrollTop] = useState(0);

  const { startIndex, endIndex, offsetY, totalHeight } = useMemo(() => {
    const safeScrollTop = Math.max(0, scrollTop);
    const total = items.length * rowHeight;
    const start = Math.max(0, Math.floor(safeScrollTop / rowHeight) - overscan);
    const visibleRows = Math.ceil(maxHeight / rowHeight) + overscan * 2;
    const end = Math.min(items.length, start + visibleRows);
    return {
      startIndex: start,
      endIndex: end,
      offsetY: start * rowHeight,
      totalHeight: total,
    };
  }, [items.length, maxHeight, overscan, rowHeight, scrollTop]);

  const visibleItems = items.slice(startIndex, endIndex);

  return (
    <div
      class={className}
      style={{ maxHeight: `${maxHeight}px`, overflowY: "auto", overflowX: "hidden" }}
      onScroll={(event) => {
        setScrollTop((event.currentTarget as HTMLDivElement).scrollTop);
      }}
    >
      <div style={{ height: `${totalHeight}px`, position: "relative" }}>
        <div style={{ transform: `translateY(${offsetY}px)` }}>
          {visibleItems.map((item, index) => {
            const actualIndex = startIndex + index;
            return (
              <div key={keyExtractor(item, actualIndex)} style={{ minHeight: `${rowHeight}px` }}>
                {renderItem(item, actualIndex)}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
