-- Schema version 2: the name check of the backup loop finds another asset with the same upload name (17 §4).
CREATE INDEX asset_upload_name_lower ON asset (lower(upload_name));
