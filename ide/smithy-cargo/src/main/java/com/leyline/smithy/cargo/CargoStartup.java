package com.leyline.smithy.cargo;

import com.intellij.openapi.project.DumbAware;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.startup.StartupActivity;

public final class CargoStartup implements StartupActivity, DumbAware {
    @Override
    public void runActivity(Project project) {
        project.getService(CargoModels.class).refresh(false);
    }
}
