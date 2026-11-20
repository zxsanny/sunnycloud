<script lang="ts">
  import { folderStore } from '$lib/files';
  import { Button, Field } from '$lib/ui';

  let { ondone }: { ondone: () => void } = $props();

  let name = $state('');
  let error = $state<string | undefined>(undefined);
  let creating = $state(false);

  const REFUSALS: Record<string, string> = {
    name_conflict: 'A file or folder with this name already exists here.',
    invalid_name: 'This name is not allowed.',
    parent_deleted: 'This folder was deleted.',
    parent_not_found: 'This folder no longer exists.',
  };

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (name.trim() === '') return;
    creating = true;
    error = undefined;
    const result = await folderStore.createFolder(name.trim());
    creating = false;
    if (result.ok) {
      ondone();
    } else {
      error = REFUSALS[result.error.code] ?? 'The folder could not be created.';
    }
  }
</script>

<form class="flex flex-wrap items-start gap-3" onsubmit={create}>
  <div class="min-w-[14rem] flex-1">
    <Field label="Folder name" bind:value={name} {error} autocomplete="off" />
  </div>
  <div class="flex gap-2 pt-[1.625rem]">
    <Button type="submit" variant="primary" loading={creating} disabled={name.trim() === ''}>
      Create
    </Button>
    <Button variant="ghost" onclick={ondone}>Cancel</Button>
  </div>
</form>
