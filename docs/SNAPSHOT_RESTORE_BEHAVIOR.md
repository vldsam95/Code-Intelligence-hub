# Snapshot Restore Behavior

VPS snapshots remain manual and external. The product does not create, verify or restore snapshots, and a snapshot is not required to pass a gate.

After a restore, the Worker recalculates the source revision. Returning to a known revision creates a `RecoveryEvent`, invalidates the currentness of later evidence and rebuilds the graph. Hub history is retained but never mixed with evidence for the restored source.

