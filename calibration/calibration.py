import os
import cv2
import numpy as np
from PyQt6.QtWidgets import (QApplication, QWidget, QVBoxLayout, QPushButton, 
                             QLabel, QFileDialog, QSlider, QHBoxLayout)
from PyQt6.QtCore import Qt

class FlagTester(QWidget):
    def __init__(self):
        super().__init__()
        self.flags_dir = os.path.join(os.path.dirname(__file__), "flags")
        self.test_image_path = None
        self.confidence = 0.70  # Чуть снизим базовый порог для геометрии
        self.init_ui()

    def init_ui(self):
        self.setWindowTitle("Тестер точности поиска флагов")
        layout = QVBoxLayout()

        self.btn_load = QPushButton("Выбрать скриншот для теста")
        self.btn_load.clicked.connect(self.load_test_image)
        layout.addWidget(self.btn_load)

        self.lbl_conf = QLabel(f"Точность (confidence): {self.confidence}")
        layout.addWidget(self.lbl_conf)
        
        slider_layout = QHBoxLayout()
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(50, 99)
        self.slider.setValue(70)
        self.slider.valueChanged.connect(self.update_confidence)
        slider_layout.addWidget(self.slider)
        layout.addWidget(self.slider)

        self.btn_run = QPushButton("ЗАПУСТИТЬ ТЕСТ")
        self.btn_run.setStyleSheet("background-color: #2ecc71; font-weight: bold;")
        self.btn_run.clicked.connect(self.run_test)
        layout.addWidget(self.btn_run)

        self.result_label = QLabel("Результат появится здесь")
        self.result_label.setWordWrap(True)
        self.result_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self.result_label)

        self.setLayout(layout)
        self.resize(450, 400)

    def update_confidence(self):
        self.confidence = self.slider.value() / 100.0
        self.lbl_conf.setText(f"Точность (confidence): {self.confidence}")

    def load_test_image(self):
        file, _ = QFileDialog.getOpenFileName(self, "Открыть скриншот", "", "Images (*.png *.jpg)")
        if file:
            self.test_image_path = file
            self.btn_load.setText(f"Файл: {os.path.basename(file)}")

    def check_specific_colors(self, crop_hsv, filename):
        """
        Проверяет, содержатся ли в вырезанном флаге цвета, 
        которые строго обязаны там быть.
        """
        # Переводим имя файла в нижний регистр для проверок
        fn = filename.lower()
        
        # Определяем диапазоны цветов в HSV
        # Синий (для Франции, США)
        lower_blue = np.array([90, 50, 50])
        upper_blue = np.array([130, 255, 255])
        
        # Зеленый (для Италии)
        lower_green = np.array([35, 50, 50])
        upper_green = np.array([85, 255, 255])

        # Красный (два диапазона, так как он на границе круга HSV)
        lower_red1 = np.array([0, 50, 50])
        upper_red1 = np.array([10, 255, 255])
        lower_red2 = np.array([170, 50, 50])
        upper_red2 = np.array([180, 255, 255])

        total_pixels = crop_hsv.shape[0] * crop_hsv.shape[1]

        if "italy" in fn or "италия" in fn:
            # В Италии ОБЯЗАН быть зеленый цвет (хотя бы 15% площади)
            mask_green = cv2.inRange(crop_hsv, lower_green, upper_green)
            green_pct = np.sum(mask_green > 0) / total_pixels
            # И НЕ должно быть синего цвета вообще (меньше 5%)
            mask_blue = cv2.inRange(crop_hsv, lower_blue, upper_blue)
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            return green_pct > 0.15 and blue_pct < 0.05

        elif "france" in fn or "франция" in fn:
            # Во Франции ОБЯЗАН быть синий цвет (хотя бы 15%)
            mask_blue = cv2.inRange(crop_hsv, lower_blue, upper_blue)
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            # И НЕ должно быть зеленого
            mask_green = cv2.inRange(crop_hsv, lower_green, upper_green)
            green_pct = np.sum(mask_green > 0) / total_pixels
            return blue_pct > 0.15 and green_pct < 0.05

        elif "usa" in fn or "сша" in fn:
            # В США должен быть и синий (крыж), и красный (полосы)
            mask_blue = cv2.inRange(crop_hsv, lower_blue, upper_blue)
            mask_red1 = cv2.inRange(crop_hsv, lower_red1, upper_red1)
            mask_red2 = cv2.inRange(crop_hsv, lower_red2, upper_red2)
            mask_red = mask_red1 + mask_red2
            
            blue_pct = np.sum(mask_blue > 0) / total_pixels
            red_pct = np.sum(mask_red > 0) / total_pixels
            
            # Синего около 10-25%, красного около 30-50%
            return blue_pct > 0.08 and red_pct > 0.25

        # Для остальных флагов пропускаем строгую проверку цветов
        return True

    def run_test(self):
        if not self.test_image_path:
            self.result_label.setText("❌ Сначала выбери картинку!")
            return
        
        if not os.path.exists(self.flags_dir):
            self.result_label.setText("❌ Папка 'flags' не найдена рядом со скриптом!")
            return

        results = []
        img_rgb = cv2.imread(self.test_image_path)
        if img_rgb is None:
            self.result_label.setText("❌ Не удалось загрузить скриншот!")
            return
            
        h_img, w_img, _ = img_rgb.shape

        for filename in os.listdir(self.flags_dir):
            if filename.endswith((".png", ".jpg")):
                template_path = os.path.join(self.flags_dir, filename)
                template = cv2.imread(template_path, 1) 
                
                if template is None: 
                    continue

                h_temp, w_temp, _ = template.shape
                if h_temp > h_img or w_temp > w_img:
                    results.append(f"{filename}: ⚠️ Ошибка (размер)")
                    continue

                # Ищем шаблон по цветной геометрии
                res = cv2.matchTemplate(img_rgb, template, cv2.TM_CCOEFF_NORMED)
                _, max_val, _, max_loc = cv2.minMaxLoc(res)

                if max_val >= self.confidence:
                    # Вырезаем совпавший кусок экрана
                    top_left = max_loc
                    crop_img = img_rgb[top_left[1]:top_left[1]+h_temp, top_left[0]:top_left[0]+w_temp]
                    crop_hsv = cv2.cvtColor(crop_img, cv2.COLOR_BGR2HSV)

                    # Вызываем умный цветовой фильтр
                    color_passed = self.check_specific_colors(crop_hsv, filename)

                    if color_passed:
                        status = "✅ НАЙДЕН"
                    else:
                        status = "❌ НЕТ (не прошел цветовой фильтр)"
                    
                    results.append(f"{filename}: {status} (совпадение: {max_val:.2f})")
                else:
                    results.append(f"{filename}: ❌ НЕТ (форма: {max_val:.2f})")

        self.result_label.setText("\n".join(results))

if __name__ == "__main__":
    app = QApplication([])
    window = FlagTester()
    window.show()
    app.exec()