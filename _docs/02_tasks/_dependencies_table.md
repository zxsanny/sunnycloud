# Dependencies Table

**Date**: 2026-11-05
**Total Tasks**: 367
**Total Complexity Points**: 1255

Recommended execution order (topological; ties by key). Test-epic refs (E-31) are noted in each task and decomposed at greenfield Step 9.

| Task | Name | Complexity | Dependencies | Epic |
|------|------|-----------|-------------|------|
| AZ-3652 | initial_structure | 5 | None | AZ-3621 |
| AZ-3681 | identity_hooks_admin_reset_spike | 2 | None | AZ-3646 |
| AZ-3695 | sharing_gaps_grant_key_spike | 2 | None | AZ-3619 |
| AZ-3705 | tree_spike_names_effect_tests | 2 | None | AZ-3640 |
| AZ-3721 | blob_rewrite_rule_spike | 3 | None | AZ-3627 |
| AZ-3722 | group_commit_fs_spike | 2 | None | AZ-3627 |
| AZ-3731 | transfer_spike_resend_stale_base | 2 | None | AZ-3643 |
| AZ-3732 | transfer_spike_nothing_to_send | 1 | None | AZ-3643 |
| AZ-3739 | sync_core_skeletons | 2 | AZ-3652 | AZ-3621 |
| AZ-3740 | migrations_setup | 2 | AZ-3652 | AZ-3621 |
| AZ-3741 | docker_compose_skeletons | 3 | AZ-3652 | AZ-3621 |
| AZ-3742 | test_harness_skeleton | 5 | AZ-3741, AZ-3652 | AZ-3621 |
| AZ-3744 | license_readme | 1 | AZ-3652 | AZ-3621 |
| AZ-3761 | ci_skeleton | 5 | AZ-3740, AZ-3741, AZ-3744, AZ-3652 | AZ-3621 |
| AZ-3763 | sc_config | 5 | AZ-3652 | AZ-3623 |
| AZ-3764 | sc_error | 3 | AZ-3652 | AZ-3626 |
| AZ-3765 | sc_clock | 3 | AZ-3761, AZ-3652 | AZ-3630 |
| AZ-3766 | sc_ids | 5 | AZ-3765, AZ-3761, AZ-3652 | AZ-3633 |
| AZ-3767 | sc_names | 5 | AZ-3765, AZ-3652 | AZ-3636 |
| AZ-3771 | media_shared_skeleton | 2 | AZ-3766, AZ-3652 | AZ-3621 |
| AZ-3773 | feed_gaps_spike | 2 | None | AZ-3635 |
| AZ-3782 | sc_db_access | 5 | AZ-3763, AZ-3764, AZ-3765, AZ-3741, AZ-3652 | AZ-3638 |
| AZ-3783 | sc_db_migrator_accounts_base | 3 | AZ-3782, AZ-3740, AZ-3741 | AZ-3638 |
| AZ-3784 | sc_audit | 5 | AZ-3782, AZ-3783, AZ-3763, AZ-3765 | AZ-3642 |
| AZ-3785 | sc_ratelimit | 3 | AZ-3763, AZ-3765, AZ-3652 | AZ-3620 |
| AZ-3786 | sc_observability | 5 | AZ-3763, AZ-3764, AZ-3765, AZ-3783, AZ-3784, AZ-3785 | AZ-3622 |
| AZ-3768 | contracts_part1 | 5 | AZ-3764, AZ-3765, AZ-3766, AZ-3786, AZ-3652 | AZ-3621 |
| AZ-3682 | identity_crate_migration_config | 3 | AZ-3768, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3652 | AZ-3646 |
| AZ-3683 | authenticator_scopes_cache | 5 | AZ-3682, AZ-3768, AZ-3764, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3684 | provider_sign_in_callbacks | 5 | AZ-3682, AZ-3763, AZ-3764, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3686 | session_refresh_rotation | 5 | AZ-3683, AZ-3684, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3687 | device_grant_exact_once | 5 | AZ-3682, AZ-3683, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3689 | mobile_code_identity_janitor | 3 | AZ-3684, AZ-3686, AZ-3687, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3690 | account_admin_and_guard | 3 | AZ-3683, AZ-3686, AZ-3687, AZ-3768, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3749 | quota_skeleton | 2 | AZ-3768, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3652 | AZ-3631 |
| AZ-3750 | quota_ledger | 3 | AZ-3749, AZ-3786 | AZ-3631 |
| AZ-3751 | quota_admin_lifecycle | 3 | AZ-3749, AZ-3750 | AZ-3631 |
| AZ-3752 | quota_recompute | 2 | AZ-3751 | AZ-3631 |
| AZ-3753 | quota_concurrency_suite | 3 | AZ-3750, AZ-3752 | AZ-3631 |
| AZ-3769 | contracts_part2 | 5 | AZ-3768 | AZ-3621 |
| AZ-3770 | contracts_part3 | 3 | AZ-3769 | AZ-3621 |
| AZ-3723 | blobstore_skeleton | 3 | AZ-3768, AZ-3770, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3652 | AZ-3627 |
| AZ-3706 | tree_skeleton | 3 | AZ-3769, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3786, AZ-3723, AZ-3652 | AZ-3640 |
| AZ-3696 | sharing_crate_migration_config | 2 | AZ-3695, AZ-3770, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3706, AZ-3652 | AZ-3619 |
| AZ-3724 | blob_file_format | 5 | AZ-3723, AZ-3766 | AZ-3627 |
| AZ-3725 | master_keyring | 5 | AZ-3723, AZ-3724, AZ-3784 | AZ-3627 |
| AZ-3727 | group_commit_barrier | 5 | AZ-3723 | AZ-3627 |
| AZ-3726 | blob_writer | 5 | AZ-3724, AZ-3725, AZ-3727 | AZ-3627 |
| AZ-3729 | blob_registry_gc_queue | 3 | AZ-3723, AZ-3724, AZ-3765 | AZ-3627 |
| AZ-3728 | blob_reader | 5 | AZ-3726, AZ-3729, AZ-3784 | AZ-3627 |
| AZ-3730 | gc_sweeper | 5 | AZ-3729, AZ-3768 | AZ-3627 |
| AZ-3733 | transfer_skeleton | 3 | AZ-3769, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3786, AZ-3723, AZ-3652 | AZ-3643 |
| AZ-3743 | orphan_sweeper | 3 | AZ-3724, AZ-3729, AZ-3765, AZ-3768 | AZ-3627 |
| AZ-3745 | scrubber | 5 | AZ-3725, AZ-3728, AZ-3730, AZ-3784, AZ-3786 | AZ-3627 |
| AZ-3746 | blob_maintenance_ports | 5 | AZ-3725, AZ-3726, AZ-3728, AZ-3729, AZ-3743, AZ-3768 | AZ-3627 |
| AZ-3747 | blobstore_perf_security | 5 | AZ-3721, AZ-3725, AZ-3726, AZ-3727, AZ-3728, AZ-3729, AZ-3745, AZ-3746 | AZ-3627 |
| AZ-3748 | blobstore_metrics_logging | 2 | AZ-3725, AZ-3727, AZ-3728, AZ-3730, AZ-3743, AZ-3745, AZ-3746, AZ-3786 | AZ-3627 |
| AZ-3774 | sync_feed_skeleton | 2 | AZ-3769, AZ-3740, AZ-3763, AZ-3764, AZ-3782, AZ-3783, AZ-3706, AZ-3652 | AZ-3635 |
| AZ-3775 | feed_stamp | 5 | AZ-3774 | AZ-3635 |
| AZ-3776 | change_feed_read | 5 | AZ-3774, AZ-3775 | AZ-3635 |
| AZ-3777 | feed_long_poll | 5 | AZ-3773, AZ-3776, AZ-3765, AZ-3786 | AZ-3635 |
| AZ-3708 | tree_folder_mutations | 5 | AZ-3706, AZ-3765, AZ-3766, AZ-3767, AZ-3775, AZ-3777, AZ-3705 | AZ-3640 |
| AZ-3709 | tree_publish_core | 5 | AZ-3706, AZ-3708, AZ-3729, AZ-3750, AZ-3775, AZ-3776, AZ-3777 | AZ-3640 |
| AZ-3707 | tree_reader | 5 | AZ-3706, AZ-3708, AZ-3709 | AZ-3640 |
| AZ-3697 | share_create_list_revoke | 3 | AZ-3696, AZ-3764, AZ-3765, AZ-3766, AZ-3784, AZ-3707 | AZ-3619 |
| AZ-3712 | tree_blob_ownership_check | 2 | AZ-3709, AZ-3729 | AZ-3640 |
| AZ-3716 | tree_revision_lookups | 3 | AZ-3707, AZ-3708, AZ-3709 | AZ-3640 |
| AZ-3717 | tree_lifecycle_blob_cleanup | 3 | AZ-3706, AZ-3708, AZ-3709, AZ-3768, AZ-3770, AZ-3729, AZ-3730 | AZ-3640 |
| AZ-3718 | tree_thumb_state_listing | 2 | AZ-3707, AZ-3770 | AZ-3640 |
| AZ-3734 | transfer_start_upload | 5 | AZ-3732, AZ-3733, AZ-3707, AZ-3766, AZ-3767, AZ-3726, AZ-3729, AZ-3750, AZ-3751 | AZ-3643 |
| AZ-3735 | transfer_put_block | 5 | AZ-3733, AZ-3734, AZ-3766, AZ-3726, AZ-3727 | AZ-3643 |
| AZ-3736 | transfer_resume_abort | 3 | AZ-3731, AZ-3734, AZ-3735, AZ-3729, AZ-3750 | AZ-3643 |
| AZ-3737 | transfer_stage_small | 3 | AZ-3733, AZ-3735, AZ-3766, AZ-3726, AZ-3729, AZ-3750, AZ-3751 | AZ-3643 |
| AZ-3778 | feed_horizon_lifecycle | 2 | AZ-3773, AZ-3774, AZ-3777, AZ-3746 | AZ-3635 |
| AZ-3685 | invite_redemption_account_creation | 5 | AZ-3681, AZ-3684, AZ-3768, AZ-3782, AZ-3783, AZ-3751, AZ-3778, AZ-3717 | AZ-3646 |
| AZ-3688 | setup_token_invites_operator_ops | 5 | AZ-3681, AZ-3685, AZ-3686, AZ-3765, AZ-3766, AZ-3782, AZ-3783 | AZ-3646 |
| AZ-3691 | identity_abuse_limits_audit | 2 | AZ-3684, AZ-3685, AZ-3686, AZ-3687, AZ-3784, AZ-3785 | AZ-3646 |
| AZ-3692 | identity_security_acceptance_suite | 3 | AZ-3683, AZ-3685, AZ-3686, AZ-3687, AZ-3688, AZ-3689, AZ-3690, AZ-3691 | AZ-3646 |
| AZ-3694 | provider_linking_and_list | 2 | AZ-3683, AZ-3684, AZ-3685 | AZ-3646 |
| AZ-3714 | tree_cascade_purge_janitors | 5 | AZ-3708, AZ-3709, AZ-3768, AZ-3729, AZ-3776, AZ-3778 | AZ-3640 |
| AZ-3698 | share_lookup_open_uniform | 5 | AZ-3695, AZ-3697, AZ-3707, AZ-3708, AZ-3709, AZ-3714 | AZ-3619 |
| AZ-3702 | share_account_erase_hook | 1 | AZ-3697, AZ-3698, AZ-3768 | AZ-3619 |
| AZ-3710 | tree_conflict_rules | 5 | AZ-3709, AZ-3714, AZ-3765, AZ-3767, AZ-3776 | AZ-3640 |
| AZ-3700 | share_authorize_content | 3 | AZ-3695, AZ-3698, AZ-3784, AZ-3708, AZ-3709, AZ-3710, AZ-3714 | AZ-3619 |
| AZ-3699 | share_password_access_grant | 5 | AZ-3700, AZ-3697, AZ-3763, AZ-3766 | AZ-3619 |
| AZ-3701 | share_abuse_limits | 3 | AZ-3695, AZ-3698, AZ-3699, AZ-3700, AZ-3784, AZ-3785 | AZ-3619 |
| AZ-3703 | sharing_security_perf_suites | 5 | AZ-3697, AZ-3698, AZ-3699, AZ-3700, AZ-3701 | AZ-3619 |
| AZ-3704 | sharing_acceptance_ref_timing | 2 | AZ-3699, AZ-3700, AZ-3701, AZ-3703 | AZ-3619 |
| AZ-3711 | tree_quota_gc_deltas | 3 | AZ-3708, AZ-3709, AZ-3710, AZ-3714, AZ-3716, AZ-3750, AZ-3751, AZ-3752, AZ-3729 | AZ-3640 |
| AZ-3713 | tree_move_vs_edit | 3 | AZ-3708, AZ-3709, AZ-3711, AZ-3776 | AZ-3640 |
| AZ-3715 | tree_restore | 5 | AZ-3709, AZ-3710, AZ-3711, AZ-3716, AZ-3729 | AZ-3640 |
| AZ-3719 | tree_security_perf_suites | 5 | AZ-3707, AZ-3708, AZ-3709, AZ-3710, AZ-3711, AZ-3714, AZ-3716, AZ-3718 | AZ-3640 |
| AZ-3738 | transfer_commit_upload | 5 | AZ-3731, AZ-3735, AZ-3736, AZ-3709, AZ-3710, AZ-3711, AZ-3712, AZ-3716, AZ-3766, AZ-3728, AZ-3729 | AZ-3643 |
| AZ-3754 | transfer_commit_batch | 5 | AZ-3737, AZ-3738, AZ-3709, AZ-3710, AZ-3711, AZ-3766, AZ-3729, AZ-3750 | AZ-3643 |
| AZ-3755 | transfer_download | 5 | AZ-3733, AZ-3738, AZ-3707, AZ-3716, AZ-3728, AZ-3729 | AZ-3643 |
| AZ-3756 | transfer_janitors_reservations | 3 | AZ-3736, AZ-3737, AZ-3738, AZ-3754, AZ-3768, AZ-3729, AZ-3750, AZ-3752, AZ-3716 | AZ-3643 |
| AZ-3757 | transfer_erase_and_crash_cases | 3 | AZ-3736, AZ-3737, AZ-3738, AZ-3754, AZ-3756, AZ-3708, AZ-3713, AZ-3768, AZ-3729 | AZ-3643 |
| AZ-3758 | transfer_security_suite | 3 | AZ-3734, AZ-3735, AZ-3736, AZ-3737, AZ-3738, AZ-3754, AZ-3755 | AZ-3643 |
| AZ-3759 | transfer_perf_metrics | 3 | AZ-3735, AZ-3737, AZ-3738, AZ-3754, AZ-3755, AZ-3786 | AZ-3643 |
| AZ-3779 | epoch_admin | 3 | AZ-3776, AZ-3778, AZ-3746, AZ-3768 | AZ-3635 |
| AZ-3780 | feed_security_acceptance | 3 | AZ-3775, AZ-3776, AZ-3777, AZ-3779, AZ-3683, AZ-3686, AZ-3687, AZ-3690, AZ-3786 | AZ-3635 |
| AZ-3781 | feed_via_tree_integration | 2 | AZ-3775, AZ-3776, AZ-3777, AZ-3709 | AZ-3635 |
| AZ-3787 | migration_test_source_spike | 2 | AZ-3783 | AZ-3647 |
| AZ-3788 | cli_skeleton | 3 | AZ-3763, AZ-3764, AZ-3786, AZ-3652 | AZ-3647 |
| AZ-3792 | janitor_scheduler | 3 | AZ-3788, AZ-3768, AZ-3765, AZ-3784, AZ-3786, AZ-3742 | AZ-3647 |
| AZ-3799 | s1_caddy_h2_spike | 3 | None | AZ-3644 |
| AZ-3800 | status_assignment_spike | 2 | None | AZ-3644 |
| AZ-3801 | crate_skeleton_accept_loop | 3 | AZ-3768, AZ-3769, AZ-3770, AZ-3763, AZ-3765, AZ-3786, AZ-3652 | AZ-3644 |
| AZ-3790 | per_command_config | 2 | AZ-3788, AZ-3763, AZ-3782, AZ-3682, AZ-3696, AZ-3723, AZ-3801 | AZ-3647 |
| AZ-3793 | migrate_commands | 3 | AZ-3787, AZ-3788, AZ-3790, AZ-3783, AZ-3742 | AZ-3647 |
| AZ-3810 | error_mapper | 3 | AZ-3800, AZ-3801, AZ-3764 | AZ-3644 |
| AZ-3814 | admin_gaps_spike | 2 | None | AZ-3624 |
| AZ-3816 | admin_crate_skeleton | 2 | AZ-3768, AZ-3763, AZ-3764, AZ-3765, AZ-3782, AZ-3783, AZ-3784, AZ-3652 | AZ-3624 |
| AZ-3817 | admin_user_management | 3 | AZ-3816, AZ-3683, AZ-3690, AZ-3751, AZ-3784 | AZ-3624 |
| AZ-3819 | account_view_deletion_request | 3 | AZ-3814, AZ-3816, AZ-3683, AZ-3690, AZ-3751, AZ-3707, AZ-3709, AZ-3697 | AZ-3624 |
| AZ-3818 | last_admin_guard | 5 | AZ-3817, AZ-3819, AZ-3690 | AZ-3624 |
| AZ-3820 | deletion_purger | 5 | AZ-3814, AZ-3816, AZ-3819, AZ-3690, AZ-3702, AZ-3717, AZ-3757, AZ-3778, AZ-3751, AZ-3730, AZ-3784, AZ-3786 | AZ-3624 |
| AZ-3821 | admin_health_aggregation | 3 | AZ-3816, AZ-3817, AZ-3768 | AZ-3624 |
| AZ-3823 | admin_security_acceptance | 3 | AZ-3817, AZ-3818, AZ-3819, AZ-3820, AZ-3821 | AZ-3624 |
| AZ-3826 | media_gaps_spike | 2 | None | AZ-3629 |
| AZ-3827 | route_registry_openapi | 5 | AZ-3801, AZ-3810, AZ-3761 | AZ-3644 |
| AZ-3802 | middleware_stack | 3 | AZ-3801, AZ-3810, AZ-3827, AZ-3764, AZ-3786 | AZ-3644 |
| AZ-3803 | body_limits_timeouts | 3 | AZ-3801, AZ-3810, AZ-3827 | AZ-3644 |
| AZ-3804 | version_gate_server_info | 3 | AZ-3810, AZ-3827 | AZ-3644 |
| AZ-3806 | auth_extraction | 3 | AZ-3810, AZ-3827, AZ-3768, AZ-3770, AZ-3766 | AZ-3644 |
| AZ-3808 | csrf_origin_check | 3 | AZ-3806, AZ-3810, AZ-3827 | AZ-3644 |
| AZ-3812 | content_header_mapper | 2 | AZ-3810, AZ-3827, AZ-3769 | AZ-3644 |
| AZ-3828 | identity_admin_handlers | 5 | AZ-3803, AZ-3806, AZ-3808, AZ-3810, AZ-3827, AZ-3768 | AZ-3644 |
| AZ-3829 | tree_transfer_feed_handlers | 5 | AZ-3803, AZ-3806, AZ-3808, AZ-3810, AZ-3812, AZ-3827, AZ-3769 | AZ-3644 |
| AZ-3762 | transfer_docs | 3 | AZ-3731, AZ-3732, AZ-3754, AZ-3755, AZ-3756, AZ-3827, AZ-3829 | AZ-3643 |
| AZ-3830 | share_media_handlers | 3 | AZ-3803, AZ-3806, AZ-3808, AZ-3810, AZ-3812, AZ-3827, AZ-3769, AZ-3770 | AZ-3644 |
| AZ-3693 | identity_scope_declarations_check | 2 | AZ-3683, AZ-3827, AZ-3828, AZ-3830 | AZ-3646 |
| AZ-3831 | internal_router_test_clock | 3 | AZ-3803, AZ-3806, AZ-3810, AZ-3827, AZ-3770, AZ-3765 | AZ-3644 |
| AZ-3832 | ops_endpoints_readiness | 3 | AZ-3801, AZ-3786, AZ-3768 | AZ-3644 |
| AZ-3811 | healthcheck_openapi_version | 2 | AZ-3788, AZ-3790, AZ-3827, AZ-3832 | AZ-3647 |
| AZ-3833 | graceful_shutdown | 2 | AZ-3801, AZ-3812, AZ-3832 | AZ-3644 |
| AZ-3834 | csrf_idor_matrices | 5 | AZ-3808, AZ-3827, AZ-3828, AZ-3829, AZ-3830 | AZ-3644 |
| AZ-3837 | media_shared_params_protocol | 3 | AZ-3770, AZ-3771, AZ-3826, AZ-3652 | AZ-3629 |
| AZ-3838 | media_policy | 3 | AZ-3837 | AZ-3629 |
| AZ-3839 | media_tables_config | 3 | AZ-3740, AZ-3782, AZ-3783, AZ-3723, AZ-3706, AZ-3763, AZ-3764, AZ-3652 | AZ-3629 |
| AZ-3840 | job_queue | 5 | AZ-3837, AZ-3839, AZ-3765, AZ-3782, AZ-3783, AZ-3786 | AZ-3629 |
| AZ-3841 | job_planner_enqueue_effect | 3 | AZ-3838, AZ-3840, AZ-3769, AZ-3709 | AZ-3629 |
| AZ-3760 | transfer_integration_pass | 2 | AZ-3705, AZ-3734, AZ-3735, AZ-3737, AZ-3738, AZ-3754, AZ-3757, AZ-3690, AZ-3841 | AZ-3643 |
| AZ-3822 | admin_dev_seed | 2 | AZ-3816, AZ-3817, AZ-3688, AZ-3708, AZ-3737, AZ-3754, AZ-3697, AZ-3751, AZ-3841 | AZ-3624 |
| AZ-3842 | derived_store_accept_input | 5 | AZ-3837, AZ-3839, AZ-3840, AZ-3826, AZ-3707, AZ-3726, AZ-3728, AZ-3729, AZ-3730 | AZ-3629 |
| AZ-3843 | worker_auth_jobs_facade | 3 | AZ-3839, AZ-3840, AZ-3842, AZ-3763, AZ-3765 | AZ-3629 |
| AZ-3844 | thumbnail_states_derived_opener | 3 | AZ-3826, AZ-3839, AZ-3840, AZ-3707, AZ-3729 | AZ-3629 |
| AZ-3720 | tree_integration_pass | 2 | AZ-3708, AZ-3709, AZ-3717, AZ-3718, AZ-3738, AZ-3754, AZ-3690, AZ-3841, AZ-3844 | AZ-3640 |
| AZ-3845 | media_janitors_erase_hook | 3 | AZ-3840, AZ-3842, AZ-3729, AZ-3706, AZ-3768 | AZ-3629 |
| AZ-3825 | admin_hook_chain_integration | 2 | AZ-3820, AZ-3845 | AZ-3624 |
| AZ-3846 | media_admin_health_doctor | 2 | AZ-3840, AZ-3842, AZ-3843, AZ-3786, AZ-3768 | AZ-3629 |
| AZ-3789 | composition_root | 5 | AZ-3788, AZ-3790, AZ-3792, AZ-3768, AZ-3769, AZ-3770, AZ-3782, AZ-3784, AZ-3785, AZ-3786, AZ-3801, AZ-3832, AZ-3681, AZ-3682, AZ-3706, AZ-3733, AZ-3723, AZ-3774, AZ-3696, AZ-3816, AZ-3749, AZ-3841, AZ-3845, AZ-3846, AZ-3702, AZ-3717, AZ-3757, AZ-3778, AZ-3751, AZ-3738, AZ-3754, AZ-3756, AZ-3730, AZ-3743, AZ-3745, AZ-3746, AZ-3714, AZ-3820, AZ-3821, AZ-3689, AZ-3779 | AZ-3647 |
| AZ-3791 | serve_start_stop | 5 | AZ-3788, AZ-3789, AZ-3790, AZ-3792, AZ-3782, AZ-3783, AZ-3725, AZ-3727, AZ-3779, AZ-3756, AZ-3801, AZ-3833, AZ-3786, AZ-3742 | AZ-3647 |
| AZ-3655 | pipeline_large_upload | 5 | AZ-3742, AZ-3801, AZ-3803, AZ-3829, AZ-3789, AZ-3791, AZ-3792, AZ-3734, AZ-3735, AZ-3736, AZ-3738, AZ-3756, AZ-3709, AZ-3711, AZ-3712, AZ-3750, AZ-3725, AZ-3726, AZ-3727, AZ-3729, AZ-3743, AZ-3775, AZ-3777, AZ-3690, AZ-3841, AZ-3652 | AZ-3647 |
| AZ-3656 | pipeline_small_upload_batch | 3 | AZ-3742, AZ-3801, AZ-3803, AZ-3829, AZ-3789, AZ-3791, AZ-3792, AZ-3737, AZ-3754, AZ-3756, AZ-3709, AZ-3711, AZ-3750, AZ-3726, AZ-3727, AZ-3729, AZ-3775, AZ-3690, AZ-3652 | AZ-3647 |
| AZ-3657 | pipeline_download | 3 | AZ-3742, AZ-3801, AZ-3812, AZ-3829, AZ-3789, AZ-3791, AZ-3755, AZ-3707, AZ-3725, AZ-3728, AZ-3655, AZ-3652 | AZ-3647 |
| AZ-3658 | pipeline_change_feed | 3 | AZ-3742, AZ-3801, AZ-3803, AZ-3829, AZ-3833, AZ-3789, AZ-3791, AZ-3775, AZ-3776, AZ-3777, AZ-3709, AZ-3683, AZ-3686, AZ-3687, AZ-3655, AZ-3656, AZ-3652 | AZ-3647 |
| AZ-3659 | pipeline_tree_conflicts | 3 | AZ-3742, AZ-3800, AZ-3801, AZ-3829, AZ-3789, AZ-3791, AZ-3708, AZ-3710, AZ-3711, AZ-3713, AZ-3750, AZ-3752, AZ-3753, AZ-3729, AZ-3775, AZ-3754, AZ-3655, AZ-3656, AZ-3652 | AZ-3647 |
| AZ-3794 | identity_operator_commands | 3 | AZ-3788, AZ-3789, AZ-3681, AZ-3688, AZ-3817, AZ-3818 | AZ-3647 |
| AZ-3664 | pipeline_sign_in | 5 | AZ-3742, AZ-3784, AZ-3785, AZ-3801, AZ-3806, AZ-3808, AZ-3828, AZ-3789, AZ-3790, AZ-3791, AZ-3794, AZ-3683, AZ-3684, AZ-3685, AZ-3686, AZ-3689, AZ-3691, AZ-3694, AZ-3751, AZ-3778, AZ-3652 | AZ-3647 |
| AZ-3795 | user_seed_audit_commands | 3 | AZ-3788, AZ-3789, AZ-3817, AZ-3819, AZ-3822, AZ-3784 | AZ-3647 |
| AZ-3797 | key_commands | 3 | AZ-3788, AZ-3789, AZ-3725, AZ-3746 | AZ-3647 |
| AZ-3798 | scrub_commands | 3 | AZ-3788, AZ-3789, AZ-3745 | AZ-3647 |
| AZ-3805 | media_commands | 2 | AZ-3788, AZ-3789, AZ-3846 | AZ-3647 |
| AZ-3660 | pipeline_media_queue_worker_api | 3 | AZ-3742, AZ-3741, AZ-3801, AZ-3806, AZ-3831, AZ-3789, AZ-3791, AZ-3792, AZ-3805, AZ-3840, AZ-3841, AZ-3843, AZ-3845, AZ-3738, AZ-3728, AZ-3707, AZ-3655, AZ-3652 | AZ-3647 |
| AZ-3807 | doctor_aggregation | 3 | AZ-3788, AZ-3789, AZ-3768 | AZ-3647 |
| AZ-3796 | epoch_restore_gc_commands | 3 | AZ-3788, AZ-3789, AZ-3807, AZ-3779, AZ-3715, AZ-3730, AZ-3765 | AZ-3647 |
| AZ-3663 | pipeline_gc_retention_restore | 3 | AZ-3742, AZ-3765, AZ-3831, AZ-3789, AZ-3791, AZ-3792, AZ-3796, AZ-3729, AZ-3730, AZ-3743, AZ-3714, AZ-3715, AZ-3717, AZ-3845, AZ-3778, AZ-3756, AZ-3659, AZ-3652 | AZ-3647 |
| AZ-3667 | pipeline_scrub | 3 | AZ-3742, AZ-3784, AZ-3786, AZ-3832, AZ-3789, AZ-3792, AZ-3798, AZ-3730, AZ-3745, AZ-3746, AZ-3821, AZ-3735, AZ-3755, AZ-3655, AZ-3663, AZ-3652 | AZ-3647 |
| AZ-3809 | doctor_root_checks | 3 | AZ-3807, AZ-3763, AZ-3782, AZ-3783, AZ-3765, AZ-3746, AZ-3716, AZ-3756, AZ-3752, AZ-3690 | AZ-3647 |
| AZ-3666 | pipeline_db_restore_resync | 3 | AZ-3742, AZ-3791, AZ-3796, AZ-3797, AZ-3807, AZ-3809, AZ-3776, AZ-3779, AZ-3725, AZ-3746, AZ-3707, AZ-3716, AZ-3755, AZ-3829, AZ-3658, AZ-3652 | AZ-3647 |
| AZ-3813 | cli_integration_suite | 3 | AZ-3788, AZ-3789, AZ-3790, AZ-3791, AZ-3792, AZ-3793, AZ-3794, AZ-3795, AZ-3796, AZ-3797, AZ-3798, AZ-3805, AZ-3807, AZ-3809, AZ-3811, AZ-3742, AZ-3761 | AZ-3647 |
| AZ-3824 | admin_performance | 3 | AZ-3817, AZ-3825, AZ-3791, AZ-3792, AZ-3828, AZ-3829 | AZ-3624 |
| AZ-3835 | integration_suite_crate_graph | 3 | AZ-3831, AZ-3832, AZ-3833, AZ-3834, AZ-3761, AZ-3791 | AZ-3644 |
| AZ-3836 | perf_security_acceptance | 3 | AZ-3799, AZ-3829, AZ-3834, AZ-3835 | AZ-3644 |
| AZ-3849 | decoder_s5_whitelist_hdr_spike | 3 | AZ-3652 | AZ-3634 |
| AZ-3850 | decoder_s4_heic_stubs_spike | 2 | AZ-3652 | AZ-3634 |
| AZ-3851 | decoder_watchdog_hook_ssim_spike | 2 | AZ-3652 | AZ-3634 |
| AZ-3852 | decoder_protocol_module | 3 | AZ-3771, AZ-3764, AZ-3652 | AZ-3634 |
| AZ-3853 | decoder_runner_socket_loop | 3 | AZ-3852, AZ-3763, AZ-3764, AZ-3786, AZ-3652 | AZ-3634 |
| AZ-3854 | decoder_process_guard | 5 | AZ-3853, AZ-3849, AZ-3763, AZ-3652 | AZ-3634 |
| AZ-3855 | decoder_containment_kill_all | 5 | AZ-3853, AZ-3854 | AZ-3634 |
| AZ-3856 | decoder_safe_open_watchdog | 3 | AZ-3853, AZ-3851 | AZ-3634 |
| AZ-3863 | decoder_media_build_stages | 5 | AZ-3741, AZ-3761, AZ-3849, AZ-3850, AZ-3652 | AZ-3634 |
| AZ-3857 | decoder_image_executor | 3 | AZ-3853, AZ-3854, AZ-3856, AZ-3850, AZ-3863, AZ-3837 | AZ-3634 |
| AZ-3858 | decoder_probe | 3 | AZ-3857, AZ-3854, AZ-3856, AZ-3849, AZ-3863 | AZ-3634 |
| AZ-3859 | decoder_video_poster | 2 | AZ-3857, AZ-3858, AZ-3849, AZ-3851 | AZ-3634 |
| AZ-3860 | decoder_video_executor | 5 | AZ-3854, AZ-3858, AZ-3849, AZ-3863, AZ-3837 | AZ-3634 |
| AZ-3861 | decoder_hello_self_check | 2 | AZ-3853, AZ-3863 | AZ-3634 |
| AZ-3862 | decoder_nvenc_probe | 2 | AZ-3860, AZ-3861 | AZ-3634 |
| AZ-3864 | decoder_image_build_checks | 3 | AZ-3863, AZ-3761 | AZ-3634 |
| AZ-3847 | media_integration_suite | 5 | AZ-3837, AZ-3838, AZ-3840, AZ-3841, AZ-3842, AZ-3843, AZ-3844, AZ-3845, AZ-3846, AZ-3801, AZ-3806, AZ-3831, AZ-3738, AZ-3730, AZ-3858, AZ-3863, AZ-3864 | AZ-3629 |
| AZ-3865 | decoder_integration_suite | 5 | AZ-3852, AZ-3853, AZ-3854, AZ-3855, AZ-3856, AZ-3857, AZ-3858, AZ-3859, AZ-3860, AZ-3861, AZ-3863, AZ-3864, AZ-3850, AZ-3851, AZ-3742 | AZ-3634 |
| AZ-3866 | decoder_perf_security_acceptance | 3 | AZ-3865 | AZ-3634 |
| AZ-3867 | worker_spike_slots_clock_scratch | 2 | AZ-3652 | AZ-3639 |
| AZ-3868 | worker_crate_skeleton | 2 | AZ-3763, AZ-3786, AZ-3741, AZ-3761, AZ-3652 | AZ-3639 |
| AZ-3869 | worker_api_client | 3 | AZ-3868, AZ-3766, AZ-3771, AZ-3837 | AZ-3639 |
| AZ-3870 | worker_scratch | 3 | AZ-3868 | AZ-3639 |
| AZ-3871 | worker_decoder_client | 3 | AZ-3868, AZ-3852 | AZ-3639 |
| AZ-3876 | worker_policy_step | 2 | AZ-3868, AZ-3838, AZ-3837, AZ-3771 | AZ-3639 |
| AZ-3872 | worker_slot_loop | 5 | AZ-3869, AZ-3870, AZ-3871, AZ-3876 | AZ-3639 |
| AZ-3772 | quota_security_perf_acceptance | 2 | AZ-3750, AZ-3751, AZ-3752, AZ-3753, AZ-3789, AZ-3791, AZ-3807, AZ-3809, AZ-3828, AZ-3829, AZ-3830, AZ-3831, AZ-3734, AZ-3736, AZ-3737, AZ-3738, AZ-3841, AZ-3842, AZ-3843, AZ-3872, AZ-3857, AZ-3860 | AZ-3631 |
| AZ-3848 | media_perf_security_acceptance | 5 | AZ-3847, AZ-3806, AZ-3829, AZ-3830, AZ-3831, AZ-3789, AZ-3791, AZ-3872, AZ-3853, AZ-3857 | AZ-3629 |
| AZ-3873 | worker_heartbeat_watchdog | 3 | AZ-3872 | AZ-3639 |
| AZ-3874 | worker_status_shutdown_main | 3 | AZ-3868, AZ-3869, AZ-3870, AZ-3871, AZ-3872, AZ-3873, AZ-3876, AZ-3765, AZ-3786 | AZ-3639 |
| AZ-3875 | worker_offset_clock_poll | 2 | AZ-3874, AZ-3867, AZ-3869, AZ-3765 | AZ-3639 |
| AZ-3877 | worker_nvenc_preference | 1 | AZ-3876, AZ-3872 | AZ-3639 |
| AZ-3661 | pipeline_media_derived_outputs | 5 | AZ-3741, AZ-3830, AZ-3789, AZ-3791, AZ-3792, AZ-3842, AZ-3844, AZ-3845, AZ-3846, AZ-3718, AZ-3755, AZ-3726, AZ-3729, AZ-3869, AZ-3870, AZ-3871, AZ-3872, AZ-3873, AZ-3874, AZ-3876, AZ-3877, AZ-3853, AZ-3855, AZ-3856, AZ-3857, AZ-3858, AZ-3859, AZ-3860, AZ-3863, AZ-3864, AZ-3660, AZ-3652 | AZ-3647 |
| AZ-3878 | worker_integration_suite | 5 | AZ-3868, AZ-3869, AZ-3870, AZ-3871, AZ-3872, AZ-3873, AZ-3874, AZ-3875, AZ-3876, AZ-3852, AZ-3853, AZ-3855, AZ-3856, AZ-3857, AZ-3858, AZ-3860, AZ-3861, AZ-3863, AZ-3840, AZ-3842, AZ-3843, AZ-3831, AZ-3789, AZ-3791 | AZ-3639 |
| AZ-3879 | worker_perf_security_acceptance | 3 | AZ-3878 | AZ-3639 |
| AZ-3881 | gate_s2_100k_files_spike | 3 | AZ-3652 | AZ-3625 |
| AZ-3882 | settings_cold_start_spike | 2 | AZ-3652 | AZ-3628 |
| AZ-3883 | windows_logon_task_spike | 2 | AZ-3652 | AZ-3625 |
| AZ-3884 | rfd_dialog_os_spike | 2 | AZ-3652 | AZ-3628 |
| AZ-3885 | linux_keyring_systemd_spike | 2 | AZ-3652 | AZ-3625 |
| AZ-3886 | screen_reader_spike | 2 | AZ-3652 | AZ-3628 |
| AZ-3887 | update_manifest_adr_spike | 3 | None | AZ-3625 |
| AZ-3889 | ipc_schema_crate | 3 | AZ-3652 | AZ-3625 |
| AZ-3888 | stub_daemon_ui_driver | 5 | AZ-3889, AZ-3652 | AZ-3628 |
| AZ-3890 | daemon_client | 5 | AZ-3889, AZ-3888, AZ-3763 | AZ-3628 |
| AZ-3913 | vault_decisions_spike | 3 | None | AZ-3648 |
| AZ-3914 | ipc_server_endpoint | 5 | AZ-3889, AZ-3652 | AZ-3625 |
| AZ-3915 | tray_autostart | 5 | AZ-3883, AZ-3889, AZ-3652 | AZ-3625 |
| AZ-3921 | macos_placeholder_tcc_spike | 2 | AZ-3652 | AZ-3625 |
| AZ-3922 | vaults_migration_config | 2 | AZ-3913, AZ-3783, AZ-3706, AZ-3740, AZ-3763, AZ-3652 | AZ-3648 |
| AZ-3923 | vault_header_service | 3 | AZ-3913, AZ-3922, AZ-3770, AZ-3707, AZ-3766, AZ-3764, AZ-3782 | AZ-3648 |
| AZ-3924 | vault_unlock_failure_report | 2 | AZ-3923, AZ-3784 | AZ-3648 |
| AZ-3925 | vault_erase_hook | 2 | AZ-3922, AZ-3768, AZ-3782 | AZ-3648 |
| AZ-3665 | pipeline_account_deletion | 3 | AZ-3742, AZ-3784, AZ-3801, AZ-3828, AZ-3789, AZ-3791, AZ-3792, AZ-3795, AZ-3818, AZ-3819, AZ-3820, AZ-3690, AZ-3717, AZ-3757, AZ-3777, AZ-3778, AZ-3702, AZ-3751, AZ-3845, AZ-3730, AZ-3925, AZ-3663, AZ-3652 | AZ-3647 |
| AZ-3926 | vault_routes_registration | 3 | AZ-3923, AZ-3924, AZ-3925, AZ-3806, AZ-3808, AZ-3810, AZ-3827, AZ-3834, AZ-3789, AZ-3764 | AZ-3648 |
| AZ-3927 | vault_membership_wiring | 3 | AZ-3913, AZ-3922, AZ-3708, AZ-3709, AZ-3734, AZ-3737, AZ-3738, AZ-3754, AZ-3697, AZ-3841 | AZ-3648 |
| AZ-3930 | vite_wasm_spike | 3 | AZ-3739, AZ-3652 | AZ-3632 |
| AZ-3931 | s12_verified_download_spike | 3 | AZ-3930, AZ-3652 | AZ-3632 |
| AZ-3932 | ios_safari_memory_spike | 2 | AZ-3930, AZ-3652 | AZ-3632 |
| AZ-3933 | setup_page_spec_spike | 1 | None | AZ-3632 |
| AZ-3934 | too_many_files_spike | 1 | AZ-3652 | AZ-3632 |
| AZ-3949 | sync_nothing_to_send_spike | 2 | AZ-3732 | AZ-3650 |
| AZ-3951 | sync_s1_client_spike | 3 | AZ-3799 | AZ-3650 |
| AZ-3953 | sync_resend_bound_spike | 2 | AZ-3731 | AZ-3650 |
| AZ-3955 | sync_core_proto | 5 | AZ-3739, AZ-3766, AZ-3764, AZ-3767, AZ-3652 | AZ-3650 |
| AZ-3957 | upload_planner | 5 | AZ-3955 | AZ-3650 |
| AZ-3958 | sync_typed_errors | 3 | AZ-3955, AZ-3764 | AZ-3650 |
| AZ-3970 | scripted_fake_server | 3 | AZ-3739, AZ-3955 | AZ-3650 |
| AZ-3967 | sync_host_traits_foundation | 5 | AZ-3955, AZ-3958, AZ-3970, AZ-3763, AZ-3765 | AZ-3650 |
| AZ-3892 | platform_fs_linux | 5 | AZ-3767, AZ-3967, AZ-3652 | AZ-3625 |
| AZ-3893 | platform_fs_windows | 5 | AZ-3892, AZ-3767, AZ-3652 | AZ-3625 |
| AZ-3959 | http_sync_client_connect | 5 | AZ-3955, AZ-3958, AZ-3967, AZ-3970 | AZ-3650 |
| AZ-3960 | sync_tree_operations | 3 | AZ-3958, AZ-3959, AZ-3967, AZ-3970 | AZ-3650 |
| AZ-3961 | sync_hash_file | 2 | AZ-3955, AZ-3959, AZ-3967 | AZ-3650 |
| AZ-3962 | large_upload_executor | 5 | AZ-3949, AZ-3957, AZ-3959, AZ-3961, AZ-3967, AZ-3970, AZ-3731 | AZ-3650 |
| AZ-3963 | small_upload_batch | 5 | AZ-3955, AZ-3958, AZ-3959, AZ-3967, AZ-3970 | AZ-3650 |
| AZ-3891 | local_index_store | 5 | AZ-3955, AZ-3963, AZ-3967, AZ-3652 | AZ-3625 |
| AZ-3894 | watcher_scanner_settler_rules | 5 | AZ-3881, AZ-3891, AZ-3892, AZ-3763, AZ-3765, AZ-3652 | AZ-3625 |
| AZ-3903 | delete_guard_holds | 3 | AZ-3891, AZ-3892, AZ-3763 | AZ-3625 |
| AZ-3901 | decide_from_disk_pair_worker | 5 | AZ-3891, AZ-3892, AZ-3894, AZ-3903, AZ-3960, AZ-3961, AZ-3962, AZ-3963, AZ-3967, AZ-3763 | AZ-3625 |
| AZ-3902 | directory_move_case_roots | 3 | AZ-3901, AZ-3891, AZ-3892, AZ-3903, AZ-3767 | AZ-3625 |
| AZ-3907 | placeholders_locks_hash_budget | 3 | AZ-3891, AZ-3892, AZ-3893, AZ-3894, AZ-3961, AZ-3763 | AZ-3625 |
| AZ-3916 | updater_verification | 5 | AZ-3887, AZ-3891, AZ-3763, AZ-3765 | AZ-3625 |
| AZ-3964 | verified_download | 3 | AZ-3955, AZ-3959, AZ-3960, AZ-3967, AZ-3970 | AZ-3650 |
| AZ-3905 | change_applier | 5 | AZ-3891, AZ-3892, AZ-3894, AZ-3901, AZ-3903, AZ-3964, AZ-3967 | AZ-3625 |
| AZ-3906 | first_sync_merger | 5 | AZ-3891, AZ-3892, AZ-3901, AZ-3905, AZ-3960, AZ-3767 | AZ-3625 |
| AZ-3918 | hostile_names_escape | 3 | AZ-3891, AZ-3901, AZ-3905, AZ-3767 | AZ-3625 |
| AZ-3965 | feed_client | 3 | AZ-3955, AZ-3958, AZ-3959, AZ-3967, AZ-3970 | AZ-3650 |
| AZ-3904 | feed_loop | 3 | AZ-3891, AZ-3905, AZ-3906, AZ-3965, AZ-3763 | AZ-3625 |
| AZ-3966 | token_lifecycle | 5 | AZ-3958, AZ-3959, AZ-3967, AZ-3970, AZ-3765 | AZ-3650 |
| AZ-3908 | token_store_device_login | 3 | AZ-3885, AZ-3891, AZ-3959, AZ-3966, AZ-3967, AZ-3763 | AZ-3625 |
| AZ-3917 | daemon_control_impl | 5 | AZ-3889, AZ-3891, AZ-3892, AZ-3894, AZ-3903, AZ-3906, AZ-3907, AZ-3908, AZ-3915, AZ-3916, AZ-3918, AZ-3959, AZ-3960 | AZ-3625 |
| AZ-3895 | settings_app_shell | 3 | AZ-3882, AZ-3886, AZ-3890, AZ-3914, AZ-3917, AZ-3652 | AZ-3628 |
| AZ-3896 | account_screen | 3 | AZ-3888, AZ-3890, AZ-3895 | AZ-3628 |
| AZ-3898 | rules_screen | 3 | AZ-3888, AZ-3890, AZ-3895 | AZ-3628 |
| AZ-3899 | settings_screen | 3 | AZ-3888, AZ-3890, AZ-3895 | AZ-3628 |
| AZ-3909 | native_dialogs | 2 | AZ-3884, AZ-3895 | AZ-3628 |
| AZ-3897 | folders_screen | 5 | AZ-3888, AZ-3890, AZ-3895, AZ-3909 | AZ-3628 |
| AZ-3900 | status_attention_screens | 5 | AZ-3888, AZ-3890, AZ-3895, AZ-3909, AZ-3915 | AZ-3628 |
| AZ-3910 | settings_release_gates | 2 | AZ-3890, AZ-3895, AZ-3896, AZ-3909, AZ-3908, AZ-3917, AZ-3761 | AZ-3628 |
| AZ-3911 | settings_ui_suite | 3 | AZ-3886, AZ-3888, AZ-3895, AZ-3896, AZ-3897, AZ-3898, AZ-3899, AZ-3900, AZ-3909, AZ-3903, AZ-3908, AZ-3914, AZ-3915, AZ-3917, AZ-3742 | AZ-3628 |
| AZ-3919 | macos_platform_fs_launchagent | 5 | AZ-3921, AZ-3892, AZ-3915, AZ-3917 | AZ-3625 |
| AZ-3912 | access_denied_message | 1 | AZ-3900, AZ-3919 | AZ-3628 |
| AZ-3920 | macos_tray_packaging | 3 | AZ-3915, AZ-3914, AZ-3919 | AZ-3625 |
| AZ-3968 | sync_uniffi_surface | 3 | AZ-3955, AZ-3958, AZ-3959, AZ-3960, AZ-3961, AZ-3962, AZ-3963, AZ-3964, AZ-3965, AZ-3966, AZ-3967 | AZ-3650 |
| AZ-3969 | sync_core_wasm_vectors | 3 | AZ-3739, AZ-3766, AZ-3955, AZ-3957, AZ-3968 | AZ-3650 |
| AZ-3971 | sync_real_server_suite | 3 | AZ-3949, AZ-3959, AZ-3960, AZ-3962, AZ-3963, AZ-3964, AZ-3965, AZ-3966, AZ-3967, AZ-3742, AZ-3804, AZ-3828, AZ-3829, AZ-3789 | AZ-3650 |
| AZ-3972 | sync_perf_acceptance | 3 | AZ-3951, AZ-3953, AZ-3959, AZ-3961, AZ-3962, AZ-3963, AZ-3964, AZ-3971 | AZ-3650 |
| AZ-3973 | ios_ac78_sibling_spike | 1 | None | AZ-3641 |
| AZ-3985 | android_uniffi_device_spike | 3 | AZ-3652 | AZ-3637 |
| AZ-3986 | play_declarations_spike | 2 | None | AZ-3637 |
| AZ-3987 | mediastore_asset_id_spike | 2 | None | AZ-3637 |
| AZ-3988 | oem_background_s9_spike | 2 | None | AZ-3637 |
| AZ-3989 | mobile_backup_checkpoint_db | 5 | AZ-3985, AZ-3967, AZ-3763, AZ-3652 | AZ-3637 |
| AZ-3974 | ios_live_photo_form_spike | 2 | AZ-3989 | AZ-3641 |
| AZ-3990 | run_backup_loop | 5 | AZ-3989, AZ-3959, AZ-3960, AZ-3961, AZ-3962, AZ-3963, AZ-3766 | AZ-3637 |
| AZ-3991 | backup_core_surface | 5 | AZ-3990, AZ-3958, AZ-3959, AZ-3967, AZ-3968, AZ-3742 | AZ-3637 |
| AZ-3997 | camera_folder_layout | 2 | AZ-3990, AZ-3991, AZ-3960, AZ-3767 | AZ-3637 |
| AZ-4002 | native_library_ci | 5 | AZ-3985, AZ-3989, AZ-3968, AZ-3761 | AZ-3637 |
| AZ-3975 | ios_uniffi_static_library | 3 | AZ-3985, AZ-3989, AZ-3991, AZ-4002, AZ-3968 | AZ-3641 |
| AZ-3983 | ios_xcuitest_harness | 3 | AZ-3975, AZ-3742 | AZ-3641 |
| AZ-4000 | flutter_channel_bridge | 3 | AZ-3991, AZ-4002 | AZ-3637 |
| AZ-4003 | android_emulator_harness | 5 | AZ-4002, AZ-3831, AZ-3742 | AZ-3637 |
| AZ-3992 | android_auth_token_vault | 5 | AZ-3991, AZ-4000, AZ-4002, AZ-4003, AZ-3966, AZ-3689 | AZ-3637 |
| AZ-3977 | ios_keychain_credentials | 2 | AZ-3975, AZ-3992, AZ-3966 | AZ-3641 |
| AZ-3993 | kotlin_asset_source | 5 | AZ-3991, AZ-3992, AZ-4000, AZ-4002, AZ-4003 | AZ-3637 |
| AZ-3994 | kotlin_entry_points | 5 | AZ-3991, AZ-3993, AZ-4000, AZ-4002, AZ-4003 | AZ-3637 |
| AZ-3995 | first_sync_job | 5 | AZ-3990, AZ-3994, AZ-4002, AZ-4003 | AZ-3637 |
| AZ-3998 | reinstall_rebuild | 5 | AZ-3990, AZ-3992, AZ-3993, AZ-3994, AZ-3997, AZ-4003, AZ-3960, AZ-3716 | AZ-3637 |
| AZ-3999 | flutter_backup_ui | 5 | AZ-3992, AZ-3993, AZ-4000, AZ-4002, AZ-4003 | AZ-3637 |
| AZ-3996 | tls_trust_bridge | 3 | AZ-3991, AZ-3992, AZ-3999, AZ-4000, AZ-4003, AZ-3968 | AZ-3637 |
| AZ-3672 | pipeline_android_sign_in | 3 | AZ-3985, AZ-3989, AZ-3991, AZ-3992, AZ-3996, AZ-3999, AZ-4000, AZ-4002, AZ-4003, AZ-3966, AZ-3968, AZ-3689, AZ-3664, AZ-3652 | AZ-3637 |
| AZ-3673 | pipeline_android_camera_backup | 5 | AZ-3987, AZ-3988, AZ-3990, AZ-3991, AZ-3993, AZ-3994, AZ-3995, AZ-3997, AZ-3998, AZ-4000, AZ-4003, AZ-3960, AZ-3961, AZ-3962, AZ-3963, AZ-3655, AZ-3656, AZ-3672, AZ-3652 | AZ-3637 |
| AZ-4001 | battery_help_diagnostics | 2 | AZ-3988, AZ-3999, AZ-4000 | AZ-3637 |
| AZ-4004 | vault_review_plan_spike | 3 | None | AZ-3645 |
| AZ-4005 | argon2_s7_benchmark_spike | 3 | AZ-3652 | AZ-3645 |
| AZ-4006 | envelope_client_meta_spike | 2 | AZ-4004 | AZ-3645 |
| AZ-4007 | vault_core_skeleton | 3 | AZ-4004, AZ-3652 | AZ-3645 |
| AZ-4008 | passphrase_policy | 3 | AZ-4007 | AZ-3645 |
| AZ-4009 | vault_header_format | 3 | AZ-4004, AZ-4005, AZ-4007 | AZ-3645 |
| AZ-4010 | vault_create_unlock | 5 | AZ-4007, AZ-4008, AZ-4009 | AZ-3645 |
| AZ-4011 | file_encryption_stream | 5 | AZ-4006, AZ-4009, AZ-4010 | AZ-3645 |
| AZ-4012 | lock_zeroize | 3 | AZ-4010, AZ-4011 | AZ-3645 |
| AZ-4013 | vault_wasm_adapter | 3 | AZ-4008, AZ-4010, AZ-4011, AZ-4012, AZ-3930 | AZ-3645 |
| AZ-4014 | vault_uniffi_adapter | 3 | AZ-4008, AZ-4010, AZ-4011, AZ-4012, AZ-3968 | AZ-3645 |
| AZ-4015 | vault_vectors_ci_gate | 3 | AZ-4009, AZ-4010, AZ-4011, AZ-4013, AZ-4014 | AZ-3645 |
| AZ-4016 | vault_fuzz_targets | 3 | AZ-4009, AZ-4011, AZ-4013 | AZ-3645 |
| AZ-4017 | vault_perf_security_tests | 3 | AZ-4005, AZ-4010, AZ-4011, AZ-4012, AZ-4013 | AZ-3645 |
| AZ-4018 | vault_acceptance_review_package | 2 | AZ-4004, AZ-4015, AZ-4016, AZ-4017 | AZ-3645 |
| AZ-3928 | vault_integration_suite | 5 | AZ-3922, AZ-3923, AZ-3924, AZ-3925, AZ-3926, AZ-3927, AZ-4009, AZ-4010, AZ-4011, AZ-4018, AZ-3820, AZ-3697, AZ-3841, AZ-3738, AZ-3755, AZ-3834, AZ-3742, AZ-3652 | AZ-3648 |
| AZ-4019 | ios_shared_crate_additions | 3 | AZ-3989, AZ-3990, AZ-3991, AZ-3974 | AZ-3637 |
| AZ-3976 | ios_photokit_asset_source | 5 | AZ-3975, AZ-3974, AZ-3990, AZ-4019 | AZ-3641 |
| AZ-3979 | ios_background_scheduling | 3 | AZ-3975, AZ-3976, AZ-3973, AZ-3990, AZ-3991 | AZ-3641 |
| AZ-3980 | ios_flutter_ui | 3 | AZ-3975, AZ-3976, AZ-3999, AZ-4000 | AZ-3641 |
| AZ-3981 | ios_icloud_optimise_opt_in | 3 | AZ-3976, AZ-3980, AZ-3990, AZ-4019 | AZ-3641 |
| AZ-4021 | release_caddyfile_security_headers | 3 | AZ-3652, AZ-3741, AZ-3742, AZ-3799, AZ-3802, AZ-3812, AZ-3827 | AZ-3621 |
| AZ-3935 | spa_scaffold | 5 | AZ-3930, AZ-3761, AZ-3741, AZ-3804, AZ-3810, AZ-3827, AZ-3652, AZ-4021 | AZ-3632 |
| AZ-3950 | playwright_web_harness | 3 | AZ-3935, AZ-3742, AZ-3652 | AZ-3632 |
| AZ-3956 | design_tokens_components | 5 | AZ-3935 | AZ-3632 |
| AZ-3936 | sign_in_setup_pages | 3 | AZ-3935, AZ-3956, AZ-3933, AZ-3950, AZ-3684, AZ-3685, AZ-3688, AZ-3828 | AZ-3632 |
| AZ-3654 | pipeline_first_run_bootstrap | 3 | AZ-3742, AZ-3782, AZ-3783, AZ-3801, AZ-3804, AZ-3828, AZ-3789, AZ-3790, AZ-3791, AZ-3793, AZ-3794, AZ-3684, AZ-3685, AZ-3688, AZ-3694, AZ-3936, AZ-3652 | AZ-3647 |
| AZ-3815 | cli_acceptance_perf_security | 5 | AZ-3813, AZ-3654, AZ-3666, AZ-3742, AZ-3761 | AZ-3647 |
| AZ-3937 | account_page | 3 | AZ-3935, AZ-3956, AZ-3950, AZ-3686, AZ-3687, AZ-3694, AZ-3818, AZ-3819, AZ-3828 | AZ-3632 |
| AZ-3938 | file_browser | 5 | AZ-3935, AZ-3956, AZ-3950, AZ-3707, AZ-3708, AZ-3829 | AZ-3632 |
| AZ-3939 | transfer_service_small_path | 5 | AZ-3935, AZ-3956, AZ-3934, AZ-3950, AZ-3708, AZ-3737, AZ-3754, AZ-3829 | AZ-3632 |
| AZ-3940 | worker_large_upload | 5 | AZ-3930, AZ-3932, AZ-3935, AZ-3939, AZ-3950, AZ-3969, AZ-3734, AZ-3735, AZ-3738, AZ-3829 | AZ-3632 |
| AZ-3941 | resume_edit_detection | 3 | AZ-3939, AZ-3940, AZ-3950, AZ-3736, AZ-3829 | AZ-3632 |
| AZ-3942 | download_strategies | 5 | AZ-3930, AZ-3931, AZ-3935, AZ-3939, AZ-3950, AZ-3969, AZ-3755, AZ-3829 | AZ-3632 |
| AZ-3944 | device_approval_page | 3 | AZ-3935, AZ-3956, AZ-3950, AZ-3687, AZ-3691, AZ-3828 | AZ-3632 |
| AZ-3668 | pipeline_desktop_device_login | 5 | AZ-3763, AZ-3885, AZ-3889, AZ-3891, AZ-3908, AZ-3914, AZ-3915, AZ-3917, AZ-3959, AZ-3966, AZ-3967, AZ-3687, AZ-3683, AZ-3828, AZ-3944, AZ-3789, AZ-3791, AZ-3652 | AZ-3625 |
| AZ-3669 | pipeline_desktop_local_to_server | 5 | AZ-3881, AZ-3891, AZ-3892, AZ-3893, AZ-3894, AZ-3901, AZ-3902, AZ-3903, AZ-3907, AZ-3914, AZ-3917, AZ-3957, AZ-3960, AZ-3961, AZ-3962, AZ-3963, AZ-3967, AZ-3655, AZ-3656, AZ-3659, AZ-3668, AZ-3652 | AZ-3625 |
| AZ-3670 | pipeline_desktop_server_to_local | 5 | AZ-3891, AZ-3892, AZ-3893, AZ-3901, AZ-3902, AZ-3903, AZ-3904, AZ-3905, AZ-3906, AZ-3918, AZ-3960, AZ-3964, AZ-3965, AZ-3657, AZ-3658, AZ-3666, AZ-3668, AZ-3652 | AZ-3625 |
| AZ-3945 | thumbnail_polling | 2 | AZ-3938, AZ-3950, AZ-3826, AZ-3844, AZ-3830 | AZ-3632 |
| AZ-3946 | admin_section | 5 | AZ-3935, AZ-3956, AZ-3950, AZ-3817, AZ-3818, AZ-3821, AZ-3688, AZ-3751, AZ-3828 | AZ-3632 |
| AZ-3947 | render_mode_hostile_content | 2 | AZ-3935, AZ-3936, AZ-3938, AZ-3939, AZ-3941, AZ-3950 | AZ-3632 |
| AZ-3943 | share_dialog_public_page | 5 | AZ-3935, AZ-3956, AZ-3938, AZ-3942, AZ-3947, AZ-3950, AZ-3697, AZ-3698, AZ-3699, AZ-3701, AZ-3830 | AZ-3632 |
| AZ-3662 | pipeline_share_links | 3 | AZ-3742, AZ-3784, AZ-3785, AZ-3801, AZ-3808, AZ-3812, AZ-3830, AZ-3789, AZ-3791, AZ-3697, AZ-3698, AZ-3699, AZ-3700, AZ-3701, AZ-3707, AZ-3708, AZ-3755, AZ-3728, AZ-3943, AZ-3655, AZ-3659, AZ-3652 | AZ-3647 |
| AZ-3948 | vault_placeholder | 1 | AZ-3935, AZ-3956, AZ-3950 | AZ-3632 |
| AZ-3952 | browser_50gib_runs | 3 | AZ-3950, AZ-3939, AZ-3940, AZ-3941, AZ-3942 | AZ-3632 |
| AZ-3671 | pipeline_web_transfer | 3 | AZ-3930, AZ-3935, AZ-3938, AZ-3939, AZ-3940, AZ-3941, AZ-3942, AZ-3945, AZ-3950, AZ-3952, AZ-3969, AZ-3655, AZ-3656, AZ-3657, AZ-3661, AZ-3652 | AZ-3632 |
| AZ-3954 | web_vault_page | 5 | AZ-3935, AZ-3956, AZ-3939, AZ-3940, AZ-3942, AZ-3948, AZ-3950, AZ-4008, AZ-4010, AZ-4011, AZ-4012, AZ-4013, AZ-3913, AZ-3923, AZ-3924, AZ-3926, AZ-3927 | AZ-3632 |
| AZ-4022 | identity_doctor_checks | 3 | AZ-3768, AZ-3682, AZ-3684, AZ-3688, AZ-3694, AZ-3807 | AZ-3646 |
| AZ-4023 | http_doctor_checks | 3 | AZ-4021, AZ-3768, AZ-3801, AZ-3802, AZ-3803, AZ-3807 | AZ-3644 |
| AZ-4024 | sign_in_with_apple_provider | 5 | AZ-3684, AZ-3682, AZ-3685, AZ-3689, AZ-3694, AZ-3691 | AZ-3646 |
| AZ-3978 | ios_auth_session_provider_rule | 3 | AZ-3975, AZ-3977, AZ-3991, AZ-3992, AZ-3689, AZ-3694, AZ-4024 | AZ-3641 |
| AZ-3982 | ios_reinstall_rebuild | 2 | AZ-3976, AZ-3978, AZ-3998, AZ-4019 | AZ-3641 |
| AZ-3984 | ios_memory_acceptance | 3 | AZ-3983, AZ-3976, AZ-3978, AZ-3979, AZ-3980 | AZ-3641 |
| AZ-3674 | pipeline_ios_photo_backup | 5 | AZ-3973, AZ-3974, AZ-3975, AZ-3976, AZ-3977, AZ-3978, AZ-3979, AZ-3980, AZ-3981, AZ-3982, AZ-3983, AZ-3984, AZ-3990, AZ-3991, AZ-3966, AZ-3968, AZ-3664, AZ-3655, AZ-3656, AZ-3652, AZ-4024 | AZ-3641 |
| AZ-4025 | mobile_vault_core_library | 3 | AZ-4002, AZ-4014, AZ-4015, AZ-4003, AZ-3968 | AZ-3637 |
| AZ-4026 | mobile_vault_operations | 5 | AZ-4025, AZ-3991, AZ-3960, AZ-3962, AZ-3963, AZ-3964, AZ-4008, AZ-4010, AZ-4011, AZ-4012, AZ-3913, AZ-3923, AZ-3924, AZ-3926, AZ-3927, AZ-4003 | AZ-3637 |
| AZ-4027 | mobile_vault_screen | 5 | AZ-4026, AZ-3999, AZ-4000, AZ-3990, AZ-4003 | AZ-3637 |
| AZ-3675 | pipeline_vault | 5 | AZ-4009, AZ-4010, AZ-4011, AZ-4012, AZ-4013, AZ-3913, AZ-3923, AZ-3924, AZ-3925, AZ-3926, AZ-3927, AZ-3928, AZ-3841, AZ-3697, AZ-3948, AZ-3655, AZ-3657, AZ-3665, AZ-3954, AZ-3652, AZ-4025, AZ-4026, AZ-4027 | AZ-3648 |
| AZ-4231 | device_login_wire | 3 | AZ-3828, AZ-3966, AZ-3959 | AZ-3650 |
| AZ-4236 | session_token_wire | 2 | AZ-4231, AZ-3828, AZ-3966 | AZ-3650 |
