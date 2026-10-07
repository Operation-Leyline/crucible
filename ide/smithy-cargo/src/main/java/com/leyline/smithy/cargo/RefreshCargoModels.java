package com.leyline.smithy.cargo;

import com.intellij.openapi.actionSystem.AnAction;
import com.intellij.openapi.actionSystem.AnActionEvent;
import com.intellij.openapi.project.DumbAware;

public final class RefreshCargoModels extends AnAction implements DumbAware {
    @Override
    public void actionPerformed(AnActionEvent event) {
        if (event.getProject() != null) event.getProject().getService(CargoModels.class).refresh(true);
    }
}
