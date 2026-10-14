<?php
declare(strict_types=1);
namespace Example\Weather\Permissions;

/**
 * The permissions `Graph` accepts (`graph`), by the values grants store.
 */
final readonly class GraphPermission
{
    /** @param string $value the value grants store; a value this client does not list is kept as is */
    public function __construct(public string $value) {}
    /**
     * Read a graph and its catalog.
     */
    public const READ = 'graph:read';
    /**
     * Change a graph: *\/ ends no comment and \[bare] links nowhere.
     * \@param is not a tag.
     */
    public const WRITE = 'graph:write';
    public const AUDIT = 'graph:audit';
    /**
     * An alias of `graph:read`.
     *
     * Deprecated: Renamed to graph:read.
     *
     * @deprecated Renamed to graph:read.
     */
    public const VIEW = 'graph:view';
    /**
     * The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.
     */
    public function canonical(): self
    {
        return match ($this->value) {
            self::VIEW => new self(self::READ),
            default => $this,
        };
    }
    public function __toString(): string { return 'GraphPermission(' . $this->value . ')'; }
}
