<?php
declare(strict_types=1);
namespace CrucibleGenerated\Runtime;

final class Credentials
{
    public static function environment(): ?AwsCredentials
    {
        $access = getenv('AWS_ACCESS_KEY_ID'); $secret = getenv('AWS_SECRET_ACCESS_KEY');
        if (!$access && !$secret) return null;
        if (!$access || !$secret) throw new \UnexpectedValueException('AWS environment credentials are incomplete');
        return new AwsCredentials($access,$secret,getenv('AWS_SESSION_TOKEN') ?: null);
    }
    public static function profile(?string $profile = null, ?string $path = null): ?AwsCredentials
    {
        $path ??= getenv('AWS_SHARED_CREDENTIALS_FILE') ?: ((getenv('HOME') ?: getenv('USERPROFILE')) . '/.aws/credentials');
        if (!is_file($path)) return null;
        $contents = file_get_contents($path);
        if ($contents === false) throw new \UnexpectedValueException('Cannot read credentials profile');
        $selected = $profile ?? (getenv('AWS_PROFILE') ?: 'default'); $section = null; $values = [];
        foreach (preg_split('/\r?\n/', $contents) as $line) {
            $line = trim($line);
            if ($line === '' || str_starts_with($line,'#') || str_starts_with($line,';')) continue;
            if (preg_match('/^\[([^\]]+)\]$/D',$line,$match)) { $section = $match[1]; continue; }
            if ($section === $selected && str_contains($line,'=')) { [$key,$value] = explode('=',$line,2); $values[trim($key)] = trim($value); }
        }
        $access = $values['aws_access_key_id'] ?? null; $secret = $values['aws_secret_access_key'] ?? null;
        if (!$access && !$secret) return null;
        if (!$access || !$secret) throw new \UnexpectedValueException('AWS profile credentials are incomplete');
        return new AwsCredentials($access,$secret,$values['aws_session_token'] ?? null);
    }
    public static function chain(\Closure ...$providers): \Closure
    {
        return static function() use ($providers): mixed { foreach ($providers as $provider) { $value = $provider(); if ($value !== null) return $value; } return null; };
    }
}
