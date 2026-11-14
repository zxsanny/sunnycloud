//! AZ-3927 AC-1 (IT-20-5, FT-P-44 step 4): every entry whose target lies in a vault, directly under its root or two
//! folders deep, new or existing, committed through the session path and the batch path, reaches the effects with
//! `vault = true` and gets no media job; the same bytes outside the vault reach them with `vault = false` and get
//! their jobs. The vault entries are committed first, so the outside jobs cannot be the reason the vault ones have
//! none. The membership comes from 02 alone: 03 passes `vault = false` on every entry.

use crate::real_ports::jpeg;
use crate::real_support::Real;
use crate::vault_support::mp4;

#[tokio::test]
async fn vault_entries_are_flagged_and_planned_no_job_while_the_same_bytes_outside_get_their_jobs()
{
    // Arrange
    let real = Real::new().await;
    let vault_root = real.make_folder(real.root, "Vault").await;
    let vault = real.create_vault(vault_root).await;
    let child = real.make_folder(vault_root, "child").await;
    let deep = real.make_folder(child, "grandchild").await;
    let (photo, clip, still, movie, edit) = (
        jpeg("photo"),
        mp4("clip"),
        jpeg("still"),
        mp4("movie"),
        jpeg("photo, edited"),
    );

    // Act
    let mut inside = vec![
        real.commit_to(vault_root, "photo.jpg", &photo, None, None)
            .await,
        real.commit_to(deep, "clip.mp4", &clip, None, None).await,
    ];
    inside.extend(
        real.batch_to(&[
            (vault_root, "still.jpg", &still),
            (deep, "movie.mp4", &movie),
        ])
        .await,
    );
    let existing = inside[0];
    inside.push(
        real.commit_to(
            vault_root,
            "photo.jpg",
            &edit,
            Some(existing.revision_id),
            None,
        )
        .await,
    );
    let mut outside = vec![
        real.commit_to(real.root, "photo.jpg", &photo, None, None)
            .await,
        real.commit_to(real.root, "clip.mp4", &clip, None, None)
            .await,
    ];
    outside.extend(
        real.batch_to(&[
            (real.root, "still.jpg", &still),
            (real.root, "movie.mp4", &movie),
        ])
        .await,
    );
    let plain = outside[0];
    outside.push(
        real.commit_to(real.root, "photo.jpg", &edit, Some(plain.revision_id), None)
            .await,
    );

    // Assert
    assert_eq!(inside[4].node_id, existing.node_id, "an existing target");
    for committed in &inside {
        assert!(real.seen_vault(committed.revision_id), "{committed:?}");
        assert_eq!(
            real.jobs_for(committed.revision_id).await,
            0,
            "{committed:?}"
        );
        assert_eq!(real.vault_of(committed.node_id).await, Some(vault));
    }
    for committed in &outside {
        assert!(!real.seen_vault(committed.revision_id), "{committed:?}");
        assert!(
            real.jobs_for(committed.revision_id).await > 0,
            "{committed:?}"
        );
        assert_eq!(real.vault_of(committed.node_id).await, None);
    }
    assert_eq!(
        [real.vault_of(child).await, real.vault_of(deep).await],
        [Some(vault), Some(vault)]
    );
    real.finish().await;
}
