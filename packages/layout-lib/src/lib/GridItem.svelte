<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * GridItem — a cell inside <Grid>. Controls how many columns it spans.
   * Class maps are static so Tailwind v4 scanner can see every candidate.
   */
  interface Props {
    /** Base column span (1..12, or "full") */
    span?: number | "full";
    spanSm?: number | "full";
    spanMd?: number | "full";
    spanLg?: number | "full";
    rowSpan?: number;
    class?: string;
    children: Snippet;
  }

  const { span = 1, spanSm, spanMd, spanLg, rowSpan, class: cls = "", children }: Props = $props();

  const SPAN: Record<string, string> = {
    "1": "col-span-1",
    "2": "col-span-2",
    "3": "col-span-3",
    "4": "col-span-4",
    "5": "col-span-5",
    "6": "col-span-6",
    "7": "col-span-7",
    "8": "col-span-8",
    "9": "col-span-9",
    "10": "col-span-10",
    "11": "col-span-11",
    "12": "col-span-12",
    full: "col-span-full",
  };
  const SPAN_SM: Record<string, string> = {
    "1": "sm:col-span-1",
    "2": "sm:col-span-2",
    "3": "sm:col-span-3",
    "4": "sm:col-span-4",
    "5": "sm:col-span-5",
    "6": "sm:col-span-6",
    "7": "sm:col-span-7",
    "8": "sm:col-span-8",
    "9": "sm:col-span-9",
    "10": "sm:col-span-10",
    "11": "sm:col-span-11",
    "12": "sm:col-span-12",
    full: "sm:col-span-full",
  };
  const SPAN_MD: Record<string, string> = {
    "1": "md:col-span-1",
    "2": "md:col-span-2",
    "3": "md:col-span-3",
    "4": "md:col-span-4",
    "5": "md:col-span-5",
    "6": "md:col-span-6",
    "7": "md:col-span-7",
    "8": "md:col-span-8",
    "9": "md:col-span-9",
    "10": "md:col-span-10",
    "11": "md:col-span-11",
    "12": "md:col-span-12",
    full: "md:col-span-full",
  };
  const SPAN_LG: Record<string, string> = {
    "1": "lg:col-span-1",
    "2": "lg:col-span-2",
    "3": "lg:col-span-3",
    "4": "lg:col-span-4",
    "5": "lg:col-span-5",
    "6": "lg:col-span-6",
    "7": "lg:col-span-7",
    "8": "lg:col-span-8",
    "9": "lg:col-span-9",
    "10": "lg:col-span-10",
    "11": "lg:col-span-11",
    "12": "lg:col-span-12",
    full: "lg:col-span-full",
  };
  const ROW_SPAN: Record<number, string> = {
    1: "row-span-1",
    2: "row-span-2",
    3: "row-span-3",
    4: "row-span-4",
    5: "row-span-5",
    6: "row-span-6",
  };

  const key = (n: number | "full" | undefined) =>
    n === undefined ? undefined : n === "full" ? "full" : String(Math.min(12, Math.max(1, n)));
</script>

<div
  class="{SPAN[key(span) ?? '1'] ?? 'col-span-1'} {spanSm !== undefined ? (SPAN_SM[key(spanSm) ?? '1'] ?? '') : ''} {spanMd !== undefined ? (SPAN_MD[key(spanMd) ?? '1'] ?? '') : ''} {spanLg !== undefined ? (SPAN_LG[key(spanLg) ?? '1'] ?? '') : ''} {rowSpan !== undefined ? (ROW_SPAN[Math.min(6, Math.max(1, rowSpan))] ?? '') : ''} {cls}"
>
  {@render children()}
</div>
