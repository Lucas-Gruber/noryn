<script setup lang="ts">
import { t as tr, formatLocale } from '../i18n';

import { computed } from 'vue';
const props = defineProps<{
  title: string;
  unit: string;
  points: { time: number; values: (number | null)[] }[];
  labels: string[];
  limit?: number;
  colors?: string[];
}>();
const palette = computed(() => props.colors || ['var(--chart-1)', 'var(--chart-2)']);
const max = computed(
  () =>
    props.limit ||
    Math.max(1, ...props.points.flatMap((p) => p.values.filter((v): v is number => v !== null))) *
      1.15,
);
const range = computed(() =>
  Math.max(1, (props.points.at(-1)?.time || 0) - (props.points[0]?.time || 0)),
);
function path(index: number) {
  let drawing = false;
  return props.points
    .map((p) => {
      const value = p.values[index];
      if (value === null || value === undefined) {
        drawing = false;
        return '';
      }
      const prefix = drawing ? 'L' : 'M';
      drawing = true;
      return `${prefix}${(12 + ((p.time - (props.points[0]?.time || 0)) / range.value) * 576).toFixed(1)},${(115 - (Math.min(max.value, value) / max.value) * 100).toFixed(1)}`;
    })
    .join(' ');
}
function value(n: number | null | undefined) {
  if (n == null) return '—';
  if (props.unit === 'KiB/s' && n >= 1024) return `${(n / 1024).toFixed(1)} MiB/s`;
  return `${n.toFixed(1)} ${props.unit}`.trim();
}

const valid = computed(() => props.points.filter((p) => p.values.some((v) => v !== null)).length);
</script>
<template>
  <article class="metric-graph">
    <div class="metric-heading">
      <strong>{{ title }}</strong>
      <div>
        <span v-for="(label, index) in labels" :key="label" :style="{ color: palette[index] }"
          >{{ label }} {{ value(points.at(-1)?.values[index]) }}</span
        >
      </div>
    </div>
    <svg
      viewBox="0 0 600 135"
      role="img"
      :aria-label="`${title}: ${labels.map((label, i) => label + ' ' + value(points.at(-1)?.values[i])).join(', ')}`"
    >
      <path
        v-for="y in [15, 65, 115]"
        :key="y"
        :d="`M12,${y} H588`"
        stroke="var(--border)"
        stroke-dasharray="3 5"
        fill="none"
      />
      <path
        v-for="(label, index) in labels"
        :key="label"
        :d="path(index)"
        :stroke="palette[index]"
        stroke-width="2.5"
        fill="none"
        stroke-linejoin="round"
        stroke-linecap="round"
      />
      <text
        v-if="valid < 2"
        x="300"
        y="75"
        text-anchor="middle"
        fill="var(--muted)"
        class="metric-placeholder"
      >
        {{ valid ? tr('Sammelt weitere Messwerte …') : tr('Wartet auf die nächste Messung …') }}
      </text>
    </svg>
    <div class="graph-time">
      <span>{{
        points.length ? new Date(points[0]!.time * 1000).toLocaleTimeString(formatLocale()) : ''
      }}</span
      ><span>{{
        points.length ? new Date(points.at(-1)!.time * 1000).toLocaleTimeString(formatLocale()) : ''
      }}</span>
    </div>
  </article>
</template>
