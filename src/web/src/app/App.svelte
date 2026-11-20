<script lang="ts">
  import { NoticeArea } from '$lib/notices';
  import { Router } from '$lib/router';
  import { notFound, routes } from './routes';
</script>

<NoticeArea />
<Router {routes} {notFound} />
