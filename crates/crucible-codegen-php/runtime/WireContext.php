<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final readonly class WireContext
{
    public function __construct(public WireCodec $codec, public string $target, public ?string $format = null, public int $depth = 0)
    {
        if ($depth > 255) throw new \UnexpectedValueException('Model nesting limit');
    }
    public function shape(): JsonObject { return $this->codec->shape($this->target); }
    public function child(JsonObject $member): self
    {
        return new self($this->codec, WireCodec::string($member->get('target')), $member->get('format'), $this->depth + 1);
    }
    public function member(string $name): self
    {
        return $this->child(WireCodec::object(WireCodec::object($this->shape()->get('members'))->get($name)));
    }
    public function read(string $name, JsonObject $object): mixed
    {
        $member = WireCodec::object(WireCodec::object($this->shape()->get('members'))->get($name));
        $value = $object->has($name) ? $object->get($name) : $member->get('default');
        if (!$object->has($name) && $value === null) throw new \UnexpectedValueException('Missing required member: ' . $name);
        return $this->codec->decode($value, $this->child($member));
    }
    public function optional(string $name, JsonObject $object): mixed
    {
        $member = WireCodec::object(WireCodec::object($this->shape()->get('members'))->get($name));
        if (!$object->has($name) && $member->get('default') === null) return Absent::Value;
        $value = $object->has($name) ? $object->get($name) : $member->get('default');
        if ($value === null) return null;
        return $this->codec->decode($value, $this->child($member));
    }
    /** @param array<string, mixed> $object */
    public function put(array &$object, string $name, mixed $value, bool $optional = false): void
    {
        if ($value === Absent::Value) return;
        if ($value === null && $optional) { $object[$name] = null; return; }
        $object[$name] = $this->codec->encode($value, $this->member($name));
    }
}
