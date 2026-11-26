---
title: The checkout carries a default ACL (other::---); a file created in it is unreadable for any other uid, also through a bind mount
applies_when: a container of the test stack (a uid other than yours, e.g. FD as 10003) reads or writes files in a bind-mounted folder of the checkout (e2e-results/...), or a new file shows mode 0660 / 0770
source: 2026-11-26, batch 129: FD could not read its script in e2e-results/fd/interactive, its record stayed empty
---

`getfacl /develop/zxsanny/sunnycloud` shows `default:user::rwx default:group::rwx default:other::---`, so every file or folder
created anywhere in the checkout inherits no access for "other": files come out 0660, folders 0770, whatever the
umask or `chmod` of the parent. A container that runs as another uid (the decoders and FD as 10003, the worker as
10002) is "other" for those files: it cannot read a file the consumer or the host wrote, and the consumer cannot read
what it wrote.

Images are not affected: `tests/consumer/Dockerfile` runs `chmod -R a+rX` after its `COPY`, and the other images build
from a context, not a mount. A bind-mounted folder that another uid must use needs its default ACL removed:
`setfacl -k -b <dir>` (tests/run.sh does this for `e2e-results/fd/<lane>` in the h2 profile). CI runners have no such ACL,
so a test that passes there can still fail here.
