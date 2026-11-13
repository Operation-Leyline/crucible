<?php
declare(strict_types=1);
namespace Example\Weather\Roles;

/**
 * The service's roles, by the values bindings store, in catalogue order.
 */
final readonly class Role
{
    /** @param string $value the value bindings store; a value this client does not list is kept as is */
    public function __construct(public string $value) {}
    /**
     * Runs the workspace.
     */
    public const OWNER = 'owner';
    /**
     * Reads notes.
     */
    public const VIEWER = 'viewer';
    /**
     * Reads, changes and writes notes.
     */
    public const EDITOR = 'editor';
    /**
     * Edits notes and reads the archives they came from.
     */
    public const ARCHIVIST = 'archivist';
    public function __toString(): string { return 'Role(' . $this->value . ')'; }
}
