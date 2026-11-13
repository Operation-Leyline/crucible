<?php
declare(strict_types=1);
namespace Example\Weather\Roles;

/**
 * The service's scopes, by the values bindings store, in catalogue order.
 */
final readonly class Scope
{
    /** @param string $value the value bindings store; a value this client does not list is kept as is */
    public function __construct(public string $value) {}
    /**
     * The workspace and everything in it.
     */
    public const WORKSPACE = 'workspace';
    /**
     * Every note in the workspace, including notes written later.
     */
    public const ALL_NOTES = 'allNotes';
    /**
     * One note.
     */
    public const NOTE = 'note';
    public function __toString(): string { return 'Scope(' . $this->value . ')'; }
}
