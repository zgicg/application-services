# The `forms` component.

This component exists for the storage, synchronization, and querying of the
`forms` engine.

The forms engine stores a number of "forms" records, and metadata associated
with them. The synced properties of forms records are just their `name`s and
`value`s, however locally we store metadata such as the creation timestamp,
last use timestamp, and usage count (all of which are local values only, as
they are not synced fields).

This component has:

- support for storage and syncing
- a somewhat rich query API (somewhat based on desktops API)
- support for expiring old forms values.
