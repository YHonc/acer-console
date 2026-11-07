<script lang="ts">
  import AnimatedNumber from './AnimatedNumber.svelte';
  let { value, suffix }: { value: number | null; suffix?: string } = $props();
</script>

<AnimatedNumber {value} {suffix} mode={suffix?.toUpperCase().includes('RPM') ? 'rpm' : 'number'} useGrouping={false} large={true} smallSuffix={true} suffixWhenUnknown={true}/>
