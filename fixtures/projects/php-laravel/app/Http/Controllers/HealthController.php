<?php

namespace App\Http\Controllers;

final class HealthController
{
    public function __invoke(): array
    {
        return ['ok' => true];
    }
}

